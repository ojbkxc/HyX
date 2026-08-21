//! BBR v2 congestion controller for quinn.
//!
//! This is a self-contained port of Cloudflare's `quiche-bbr2` (itself a
//! port of Google's BBRv2 for TCP/QUIC) adapted to the [`Controller`]
//! event model of quinn. BBR v2's philosophy differs from BBR v1: instead of
//! blindly multiplying inflight by a fixed pacing gain to probe for
//! bandwidth, it tracks an explicit `inflight_hi` upper bound and an
//! `inflight_lo`/`bandwidth_lo` lower bound and oscillates the sending rate
//! between them, so it converges on the true bottleneck bandwidth while
//! keeping queues (and therefore latency and packet loss) low.
//!
//! State machine (mirrors quiche-bbr2):
//!
//! * `Startup` — gain 2.773/2.0, exits on bandwidth plateau or excessive loss
//!   into `Drain`.
//! * `Drain` — drains the startup queue down to one BDP, then enters
//!   `ProbeBw`.
//! * `ProbeBw` — cycles `Up(1.25) → Down(0.9) → Cruise(1.0) → Refill(1.0)`,
//!   adapting `inflight_hi` (probe too high → lowered) and `inflight_lo`
//!   (losses raise it). Enters `ProbeRtt` when the min-RTT is stale.
//! * `ProbeRtt` — drops inflight to 0.5 BDP for 200 ms to re-measure min-RTT.
//!
//! The quinn [`Controller`] signal is coarser than quiche's (packet-level
//! bandwidth sampling is unavailable), so bandwidth estimation reuses the
//! send/ack-rate window filter from quinn's BBR v1, and per-round loss
//! accounting approximates quiche's `BandwidthSampler`. The mode/gain logic
//! and all constants are carried over faithfully from quiche-bbr2.

use std::sync::Arc;

use crate::connection::RttEstimator;
use crate::{Duration, Instant};

use super::bbr::bw_estimation::BandwidthEstimation;
use super::{
    BASE_DATAGRAM_SIZE, Controller, ControllerFactory, ControllerMetrics,
};

// ---- BBRv2 parameters (quiche-bbr2 DEFAULT_PARAMS) ----
const STARTUP_CWND_GAIN: f32 = 2.0;
const STARTUP_PACING_GAIN: f32 = 2.773;
const HIGH_GAIN: f32 = 2.885;
const FULL_BW_THRESHOLD: f32 = 1.25;
const STARTUP_FULL_BW_ROUNDS: u32 = 3;
const STARTUP_FULL_LOSS_COUNT: u32 = 8;
const DRAIN_CWND_GAIN: f32 = 2.0;
const DRAIN_PACING_GAIN: f32 = 1.0 / HIGH_GAIN;
const PROBE_BW_PROBE_MAX_ROUNDS: u32 = 63;
const PROBE_BW_PROBE_BASE_DURATION: Duration = Duration::from_millis(2000);
const PROBE_BW_FULL_LOSS_COUNT: u32 = 2;
const PROBE_BW_PROBE_UP_PACING_GAIN: f32 = 1.25;
const PROBE_BW_PROBE_DOWN_PACING_GAIN: f32 = 0.9;
const PROBE_BW_DEFAULT_PACING_GAIN: f32 = 1.0;
const PROBE_BW_CWND_GAIN: f32 = 2.0;
const PROBE_BW_UP_CWND_GAIN: f32 = 2.25;
const MAX_PROBE_UP_QUEUE_ROUNDS: u32 = 2;
const PROBE_RTT_INFLIGHT_TARGET_BDP_FRACTION: f32 = 0.5;
const PROBE_RTT_PERIOD: Duration = Duration::from_millis(10_000);
const PROBE_RTT_DURATION: Duration = Duration::from_millis(200);
const PROBE_RTT_PACING_GAIN: f32 = 1.0;
const PROBE_RTT_CWND_GAIN: f32 = 1.0;
const INFLIGHT_HI_HEADROOM: f32 = 0.15;
const LOSS_THRESHOLD: f32 = 0.015;
const BETA: f32 = 0.3;
const DEFAULT_MSS: u64 = 1300;
const INITIAL_RTT: Duration = Duration::from_millis(50);

const MAX_MODE_CHANGES_PER_EVENT: u32 = 4;

/// Maximum number of modes a connection can enter probe-rtt scheduling.
const INFINITE: u64 = u64::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Startup,
    Drain,
    ProbeBw,
    ProbeRtt,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CyclePhase {
    NotStarted,
    Up,
    Down,
    Cruise,
    Refill,
}

impl CyclePhase {
    fn pacing_gain(self) -> f32 {
        match self {
            CyclePhase::Up => PROBE_BW_PROBE_UP_PACING_GAIN,
            CyclePhase::Down => PROBE_BW_PROBE_DOWN_PACING_GAIN,
            _ => PROBE_BW_DEFAULT_PACING_GAIN,
        }
    }

    fn cwnd_gain(self) -> f32 {
        match self {
            CyclePhase::Up => PROBE_BW_UP_CWND_GAIN,
            _ => PROBE_BW_CWND_GAIN,
        }
    }
}

/// A single congestion event synthesized from quinn's ack/loss callbacks.
struct Event {
    now: Instant,
    prior_cwnd: u64,
    prior_bytes_in_flight: u64,
    bytes_in_flight: u64,
    bytes_acked: u64,
    bytes_lost: u64,
    end_of_round_trip: bool,
    is_probing_for_bandwidth: bool,
    is_app_limited: bool,
}

/// BBR v2 congestion controller.
pub struct Bbr2 {
    config: Arc<BbrConfig>,
    current_mtu: u64,
    mss: u64,

    // Model
    max_bandwidth: BandwidthEstimation,
    min_rtt: Duration,
    latest_rtt: Duration,
    min_rtt_timestamp: Instant,

    // State machine
    mode: Mode,
    phase: CyclePhase,
    full_bandwidth_reached: bool,
    full_bandwidth_baseline: u64,
    rounds_without_bandwidth_growth: u32,
    inflight_hi: u64,
    inflight_lo: u64,
    bandwidth_lo: Option<u64>,

    // cwnd / pacing
    cwnd: u64,
    init_cwnd: u64,
    min_cwnd: u64,
    cwnd_lo: u64,
    cwnd_hi: u64,
    pacing_gain: f32,
    cwnd_gain: f32,
    pacing_rate: u64,

    // Accounting
    acked_bytes: u64,
    bytes_lost_in_round: u64,
    loss_events_in_round: u32,
    min_bytes_in_flight_in_round: u64,
    pending_lost_bytes: u64,
    prev_in_flight_count: u64,
    last_sample_is_app_limited: bool,

    // Round tracking
    round_count: u64,
    current_round_trip_end_packet_number: u64,
    max_sent_packet_number: u64,
    max_acked_packet_number: u64,

    // ProbeBW cycle
    cycle_start_time: Option<Instant>,
    phase_start_time: Option<Instant>,
    rounds_in_phase: u32,
    rounds_since_probe: u32,
    probe_wait_time: Option<Duration>,
    probe_up_rounds: u32,
    probe_up_bytes: Option<u64>,
    probe_up_acked: u64,
    is_sample_from_probing: bool,
    has_advanced_max_bw: bool,
    last_cycle_probed_too_high: bool,
    last_cycle_stopped_risky_probe: bool,

    // ProbeRTT
    exit_probe_rtt_at: Option<Instant>,
    probe_rtt_last_started_at: Option<Instant>,
}

impl Bbr2 {
    fn new(config: Arc<BbrConfig>, current_mtu: u16) -> Self {
        let mtu = current_mtu as u64;
        let cwnd = config.initial_window.max(4 * mtu);
        Self {
            config,
            current_mtu: mtu,
            mss: mtu.min(DEFAULT_MSS),
            max_bandwidth: BandwidthEstimation::default(),
            min_rtt: INITIAL_RTT,
            latest_rtt: INITIAL_RTT,
            min_rtt_timestamp: Instant::now(),
            mode: Mode::Startup,
            phase: CyclePhase::NotStarted,
            full_bandwidth_reached: false,
            full_bandwidth_baseline: 0,
            rounds_without_bandwidth_growth: 0,
            inflight_hi: INFINITE,
            inflight_lo: INFINITE,
            bandwidth_lo: None,
            cwnd,
            init_cwnd: cwnd,
            min_cwnd: 4 * mtu,
            cwnd_lo: cwnd,
            cwnd_hi: INFINITE,
            pacing_gain: STARTUP_PACING_GAIN,
            cwnd_gain: STARTUP_CWND_GAIN,
            pacing_rate: (bw_from_delta(cwnd, INITIAL_RTT) as f64 * HIGH_GAIN as f64) as u64,
            acked_bytes: 0,
            bytes_lost_in_round: 0,
            loss_events_in_round: 0,
            min_bytes_in_flight_in_round: INFINITE,
            pending_lost_bytes: 0,
            prev_in_flight_count: 0,
            last_sample_is_app_limited: false,
            round_count: 0,
            current_round_trip_end_packet_number: 0,
            max_sent_packet_number: 0,
            max_acked_packet_number: 0,
            cycle_start_time: None,
            phase_start_time: None,
            rounds_in_phase: 0,
            rounds_since_probe: 0,
            probe_wait_time: None,
            probe_up_rounds: 0,
            probe_up_bytes: None,
            probe_up_acked: 0,
            is_sample_from_probing: false,
            has_advanced_max_bw: false,
            last_cycle_probed_too_high: false,
            last_cycle_stopped_risky_probe: false,
            exit_probe_rtt_at: None,
            probe_rtt_last_started_at: None,
        }
    }

    // ---- model helpers ----
    fn bandwidth_estimate(&self) -> u64 {
        let max_bw = self.max_bandwidth.get_estimate();
        match self.bandwidth_lo {
            Some(lo) if lo < max_bw => lo,
            _ => max_bw,
        }
    }

    fn bdp(&self, bandwidth: u64, gain: f32) -> u64 {
        (bandwidth as f64 * gain as f64 * self.min_rtt.as_secs_f64()) as u64
    }

    fn target_cwnd(&self, gain: f32) -> u64 {
        self.bdp(self.bandwidth_estimate(), gain).max(self.cwnd_lo)
    }

    fn is_min_rtt_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.min_rtt_timestamp) > PROBE_RTT_PERIOD
    }

    fn inflight_hi_headroom(&self) -> u64 {
        let headroom = (self.inflight_hi as f32 * INFLIGHT_HI_HEADROOM) as u64;
        self.inflight_hi.saturating_sub(headroom)
    }

    fn is_inflight_hi_limited(&self, bytes_in_flight: u64) -> bool {
        bytes_in_flight >= self.inflight_hi
    }

    fn bytes_lost_too_high(&self) -> bool {
        // Approximate quiche's is_inflight_too_high: the send-state inflight
        // at send time is not trackable here, so use the inflight at the
        // start of the round as the reference.
        let inflight_at_send = self.prev_in_flight_count.max(1);
        self.bytes_lost_in_round
            > (inflight_at_send as f32 * LOSS_THRESHOLD) as u64
    }

    // ---- lifecycle / round helpers ----
    fn on_new_round(&mut self) {
        self.bytes_lost_in_round = 0;
        self.loss_events_in_round = 0;
        self.min_bytes_in_flight_in_round = INFINITE;
    }

    fn restart_round_early(&mut self) {
        self.on_new_round();
        self.rounds_without_bandwidth_growth = 0;
    }

    fn has_bandwidth_growth(&mut self) -> bool {
        let threshold = self.full_bandwidth_baseline as f64 * FULL_BW_THRESHOLD as f64;
        let max_bw = self.max_bandwidth.get_estimate();
        if max_bw as f64 >= threshold {
            self.full_bandwidth_baseline = max_bw;
            self.rounds_without_bandwidth_growth = 0;
            return true;
        }

        if self.last_sample_is_app_limited {
            return false;
        }

        self.rounds_without_bandwidth_growth += 1;
        if self.rounds_without_bandwidth_growth >= STARTUP_FULL_BW_ROUNDS {
            self.full_bandwidth_reached = true;
        }
        false
    }

    // ---- ProbeBW cycle transitions ----
    fn enter_probe_down(&mut self, probed_too_high: bool, stopped_risky: bool, now: Instant) {
        self.last_cycle_probed_too_high = probed_too_high;
        self.last_cycle_stopped_risky_probe = stopped_risky;
        self.phase = CyclePhase::Down;
        self.cycle_start_time = Some(now);
        self.phase_start_time = Some(now);
        self.rounds_in_phase = 0;
        self.rounds_since_probe = 0;
        self.probe_wait_time =
            Some(PROBE_BW_PROBE_BASE_DURATION + Duration::from_micros(500));
        self.probe_up_bytes = None;
        self.has_advanced_max_bw = false;
        self.is_sample_from_probing = false;
        self.restart_round_early();
    }

    fn enter_probe_cruise(&mut self, now: Instant) {
        if self.phase == CyclePhase::Down {
            self.exit_probe_down();
        }
        self.cap_inflight_lo(self.inflight_hi);
        self.phase = CyclePhase::Cruise;
        self.phase_start_time = Some(now);
        self.rounds_in_phase = 0;
        self.is_sample_from_probing = false;
    }

    fn enter_probe_refill(&mut self, now: Instant) {
        if self.phase == CyclePhase::Down {
            self.exit_probe_down();
        }
        self.phase = CyclePhase::Refill;
        self.phase_start_time = Some(now);
        self.rounds_in_phase = 0;
        self.is_sample_from_probing = false;
        self.last_cycle_stopped_risky_probe = false;
        self.bandwidth_lo = None;
        self.inflight_lo = INFINITE;
        self.probe_up_acked = 0;
        self.restart_round_early();
    }

    fn enter_probe_up(&mut self, now: Instant, cwnd: u64) {
        self.phase = CyclePhase::Up;
        self.phase_start_time = Some(now);
        self.rounds_in_phase = 0;
        self.is_sample_from_probing = true;
        self.raise_inflight_high_slope(cwnd);
        self.restart_round_early();
    }

    fn exit_probe_down(&mut self) {
        // No-op: BandwidthEstimation keeps its own windowed max filter.
        self.has_advanced_max_bw = true;
    }

    fn cap_inflight_lo(&mut self, cap: u64) {
        if self.inflight_lo != INFINITE {
            self.inflight_lo = self.inflight_lo.min(cap);
        }
    }

    fn raise_inflight_high_slope(&mut self, cwnd: u64) {
        let growth = 1u64 << self.probe_up_rounds.min(30);
        self.probe_up_rounds = self.probe_up_rounds.saturating_add(1).min(30);
        let probe_up_bytes = cwnd / growth;
        self.probe_up_bytes = Some(probe_up_bytes.max(DEFAULT_MSS));
    }

    // ---- ProbeBW per-phase updates ----
    fn maybe_adapt_upper_bounds(&mut self, evt: &Event) -> bool {
        // Returns true when the probe was detected "too high".
        if self.is_sample_from_probing {
            if self.loss_events_in_round >= PROBE_BW_FULL_LOSS_COUNT
                && self.bytes_lost_too_high()
            {
                self.is_sample_from_probing = false;
                if !evt.is_app_limited {
                    let inflight_target =
                        (evt.prior_bytes_in_flight as f32 * (1.0 - BETA)) as u64;
                    self.inflight_hi =
                        evt.prior_bytes_in_flight.max(inflight_target);
                }
                return true;
            }
            return false;
        }

        if self.inflight_hi != INFINITE && evt.prior_bytes_in_flight > self.inflight_hi {
            self.inflight_hi = evt.prior_bytes_in_flight;
        }
        false
    }

    fn update_probe_down(&mut self, evt: &Event) {
        if self.rounds_in_phase == 1 && evt.end_of_round_trip {
            self.is_sample_from_probing = false;
            if self.last_cycle_stopped_risky_probe && !self.last_cycle_probed_too_high {
                self.enter_probe_refill(evt.now);
                return;
            }
        }

        if self.maybe_adapt_upper_bounds(evt) {
            return;
        }

        if self.is_time_to_probe_bandwidth(evt) {
            self.enter_probe_refill(evt.now);
            return;
        }

        // Stay in PROBE_DOWN at most a min-RTT.
        if let Some(phase_start) = self.phase_start_time {
            if evt.now.saturating_duration_since(phase_start) > self.min_rtt {
                self.enter_probe_cruise(evt.now);
                return;
            }
        }

        if evt.bytes_in_flight > self.inflight_hi_headroom() {
            return;
        }
        // Drain to BDP → Cruise.
        if evt.bytes_in_flight < self.bdp(self.max_bandwidth.get_estimate(), 1.0) {
            self.enter_probe_cruise(evt.now);
        }
    }

    fn update_probe_cruise(&mut self, evt: &Event) {
        self.maybe_adapt_upper_bounds(evt);
        if self.is_time_to_probe_bandwidth(evt) {
            self.enter_probe_refill(evt.now);
        }
    }

    fn update_probe_refill(&mut self, evt: &Event) {
        self.maybe_adapt_upper_bounds(evt);
        if self.rounds_in_phase > 0 && evt.end_of_round_trip {
            self.enter_probe_up(evt.now, evt.prior_cwnd);
        }
    }

    fn update_probe_up(&mut self, evt: &Event) {
        if self.maybe_adapt_upper_bounds(evt) {
            self.enter_probe_down(true, false, evt.now);
            return;
        }

        self.probe_inflight_high_upward(evt);

        let mut risky = self.last_cycle_probed_too_high
            && evt.prior_bytes_in_flight >= self.inflight_hi;
        if !risky && self.rounds_in_phase > 0 {
            // Queueing threshold: full_bw_threshold * BDP + 2 mss
            let queuing_threshold =
                (FULL_BW_THRESHOLD * self.bdp(self.max_bandwidth.get_estimate(), 1.0) as f32)
                    as u64
                    + 2 * DEFAULT_MSS;
            risky = evt.bytes_in_flight >= queuing_threshold;
        }

        if risky {
            self.enter_probe_down(false, true, evt.now);
        }
    }

    fn probe_inflight_high_upward(&mut self, evt: &Event) {
        if evt.prior_bytes_in_flight < evt.prior_cwnd
            || evt.prior_cwnd < self.inflight_hi
        {
            return;
        }

        self.probe_up_acked += evt.bytes_acked;

        if let Some(probe_up_bytes) = self.probe_up_bytes {
            if self.probe_up_acked >= probe_up_bytes {
                let delta = self.probe_up_acked / probe_up_bytes;
                self.probe_up_acked -= delta * probe_up_bytes;
                self.inflight_hi = self
                    .inflight_hi
                    .saturating_add(delta * DEFAULT_MSS);
            }
        }

        if evt.end_of_round_trip {
            self.raise_inflight_high_slope(evt.prior_cwnd);
        }
    }

    fn is_time_to_probe_bandwidth(&self, evt: &Event) -> bool {
        let wait_elapsed = self
            .probe_wait_time
            .and_then(|d| self.cycle_start_time.map(|t| evt.now.saturating_duration_since(t) > d))
            .unwrap_or(false);
        if wait_elapsed {
            return true;
        }

        // Reno coexistence: probe every min(63, BDP*BDP gain / mss) rounds.
        let reno_rounds = (self.bdp(self.target_cwnd(1.0), 1.0) / DEFAULT_MSS)
            .min(PROBE_BW_PROBE_MAX_ROUNDS as u64);
        self.rounds_since_probe as u64 >= reno_rounds
    }

    // ---- mode processing ----
    fn enter_mode(&mut self, mode: Mode, now: Instant) {
        self.mode = mode;
        self.phase_start_time = Some(now);
        self.exit_probe_rtt_at = None;
        self.pacing_gain = match mode {
            Mode::Startup => STARTUP_PACING_GAIN,
            Mode::Drain => DRAIN_PACING_GAIN,
            Mode::ProbeBw => self.phase.pacing_gain(),
            Mode::ProbeRtt => PROBE_RTT_PACING_GAIN,
        };
        self.cwnd_gain = match mode {
            Mode::Startup => STARTUP_CWND_GAIN,
            Mode::Drain => DRAIN_CWND_GAIN,
            Mode::ProbeBw => self.phase.cwnd_gain(),
            Mode::ProbeRtt => PROBE_RTT_CWND_GAIN,
        };
        if mode == Mode::ProbeBw && self.phase == CyclePhase::NotStarted {
            self.enter_probe_down(false, false, now);
        }
    }

    fn poll_event(&mut self, evt: &Event) {
        match self.mode {
            Mode::Startup => self.poll_startup(evt),
            Mode::Drain => self.poll_drain(evt),
            Mode::ProbeBw => self.poll_probe_bw(evt),
            Mode::ProbeRtt => self.poll_probe_rtt(evt),
        }
    }

    fn poll_startup(&mut self, evt: &Event) {
        if self.full_bandwidth_reached {
            self.bandwidth_lo = None;
            self.enter_mode(Mode::Drain, evt.now);
            return;
        }
        if !evt.end_of_round_trip {
            return;
        }

        let growth = self.has_bandwidth_growth();
        if !growth && !evt.is_app_limited && !self.full_bandwidth_reached {
            if self.loss_events_in_round >= STARTUP_FULL_LOSS_COUNT
                && self.bytes_lost_too_high()
            {
                let mut new_hi = self.bdp(self.max_bandwidth.get_estimate(), 1.0);
                if self.bytes_lost_in_round > new_hi {
                    new_hi = self.bytes_lost_in_round;
                }
                self.inflight_hi = new_hi;
                self.full_bandwidth_reached = true;
            }
        }

        if self.full_bandwidth_reached {
            self.bandwidth_lo = None;
            self.enter_mode(Mode::Drain, evt.now);
        }
    }

    fn poll_drain(&mut self, evt: &Event) {
        if evt.bytes_in_flight <= self.bdp(self.max_bandwidth.get_estimate(), 1.0) {
            self.enter_mode(Mode::ProbeBw, evt.now);
        }
    }

    fn poll_probe_bw(&mut self, evt: &Event) {
        if evt.end_of_round_trip {
            if self.phase_start_time != Some(evt.now) {
                self.rounds_in_phase += 1;
            }
            if self.cycle_start_time != Some(evt.now) {
                self.rounds_since_probe += 1;
            }
        }

        let mut switch_to_probe_rtt = false;
        match self.phase {
            CyclePhase::Up => self.update_probe_up(evt),
            CyclePhase::Down => {
                let prev_phase = self.phase;
                self.update_probe_down(evt);
                if self.phase != prev_phase && self.is_min_rtt_expired(evt.now) {
                    switch_to_probe_rtt = true;
                }
            }
            CyclePhase::Cruise => self.update_probe_cruise(evt),
            CyclePhase::Refill => self.update_probe_refill(evt),
            CyclePhase::NotStarted => {}
        }

        if switch_to_probe_rtt {
            self.enter_mode(Mode::ProbeRtt, evt.now);
        } else if self.mode == Mode::ProbeBw {
            self.pacing_gain = self.phase.pacing_gain();
            self.cwnd_gain = self.phase.cwnd_gain();
        }
    }

    fn poll_probe_rtt(&mut self, evt: &Event) {
        if let Some(exit) = self.exit_probe_rtt_at {
            if evt.now > exit {
                self.enter_mode(Mode::ProbeBw, evt.now);
                return;
            }
            return;
        }

        let inflight_target =
            self.bdp(self.max_bandwidth.get_estimate(), PROBE_RTT_INFLIGHT_TARGET_BDP_FRACTION);
        if evt.bytes_in_flight <= inflight_target {
            self.exit_probe_rtt_at = Some(evt.now + PROBE_RTT_DURATION);
            self.probe_rtt_last_started_at = Some(evt.now);
        }
    }

    fn run_congestion_event(&mut self, evt: &mut Event) {
        for _ in 0..MAX_MODE_CHANGES_PER_EVENT {
            let before = self.mode;
            self.poll_event(evt);
            if self.mode != before {
                if self.mode == Mode::ProbeRtt {
                    break;
                }
            } else {
                break;
            }
        }

        self.update_pacing_rate(evt.bytes_acked);
        self.update_congestion_window(evt.bytes_acked);
        self.prev_in_flight_count = evt.bytes_in_flight;
        self.last_sample_is_app_limited = evt.is_app_limited;
        if evt.end_of_round_trip {
            self.on_new_round();
        }
    }

    fn update_pacing_rate(&mut self, _bytes_acked: u64) {
        let bw = self.bandwidth_estimate();
        if bw == 0 {
            return;
        }
        let target_rate = bw as f64 * self.pacing_gain as f64;
        if self.full_bandwidth_reached {
            self.pacing_rate = target_rate as u64;
        } else {
            // Keep raising the pacing rate through startup; never drop it.
            self.pacing_rate = (self.pacing_rate as f64).max(target_rate) as u64;
        }
    }

    fn update_congestion_window(&mut self, bytes_acked: u64) {
        let mut target = self.target_cwnd(self.cwnd_gain);
        let prior = self.cwnd;
        if self.full_bandwidth_reached {
            self.cwnd = target.min(prior + bytes_acked);
        } else if prior < target || prior < 2 * self.init_cwnd {
            self.cwnd = prior + bytes_acked;
        }

        // Mode cwnd limits (inflight_hi/lo).
        let lo = if self.inflight_lo != INFINITE { self.inflight_lo } else { 0 };
        let hi = match self.mode {
            Mode::Startup | Mode::Drain => self.inflight_lo,
            Mode::ProbeBw => match self.phase {
                CyclePhase::Cruise => {
                    self.inflight_lo.min(self.inflight_hi_headroom())
                }
                CyclePhase::Up => self.inflight_lo,
                _ => self.inflight_lo.min(self.inflight_hi),
            },
            Mode::ProbeRtt => self
                .inflight_lo
                .min(self.inflight_hi_headroom())
                .min(self.bdp(
                    self.max_bandwidth.get_estimate(),
                    PROBE_RTT_INFLIGHT_TARGET_BDP_FRACTION,
                )),
        };
        self.cwnd = self.cwnd.max(lo).min(hi);
        self.cwnd = self.cwnd.max(self.min_cwnd).min(self.cwnd_hi);
        let _ = target;

        self.cwnd = self.cwnd.max(self.min_cwnd);
    }
}

impl Controller for Bbr2 {
    fn on_sent(&mut self, now: Instant, bytes: u64, last_packet_number: u64) {
        self.max_sent_packet_number = last_packet_number;
        self.max_bandwidth.on_sent(now, bytes);
    }

    fn on_ack(
        &mut self,
        now: Instant,
        sent: Instant,
        bytes: u64,
        app_limited: bool,
        rtt: &RttEstimator,
    ) {
        self.max_bandwidth.on_ack(now, sent, bytes, self.round_count, app_limited);
        self.acked_bytes += bytes;
        self.latest_rtt = rtt.get();
        let min = rtt.min();
        if min < self.min_rtt {
            self.min_rtt = min;
            self.min_rtt_timestamp = now;
        }
    }

    fn on_end_acks(
        &mut self,
        now: Instant,
        in_flight: u64,
        app_limited: bool,
        largest_packet_num_acked: Option<u64>,
    ) {
        let bytes_acked = self.max_bandwidth.bytes_acked_this_window();
        self.max_bandwidth.end_acks(self.round_count, app_limited);
        if let Some(largest_acked) = largest_packet_num_acked {
            self.max_acked_packet_number = largest_acked;
        }

        let mut is_round_start = false;
        if bytes_acked > 0
            && self.max_acked_packet_number > self.current_round_trip_end_packet_number
        {
            is_round_start = true;
            self.current_round_trip_end_packet_number = self.max_sent_packet_number;
            self.round_count += 1;
        }

        let event_bytes_lost = std::mem::take(&mut self.pending_lost_bytes);
        self.min_bytes_in_flight_in_round =
            self.min_bytes_in_flight_in_round.min(in_flight);

        let mut evt = Event {
            now,
            prior_cwnd: self.cwnd,
            prior_bytes_in_flight: self.prev_in_flight_count,
            bytes_in_flight: in_flight,
            bytes_acked,
            bytes_lost: event_bytes_lost,
            end_of_round_trip: is_round_start,
            is_probing_for_bandwidth: self.mode == Mode::Startup
                || (self.mode == Mode::ProbeBw
                    && (self.phase == CyclePhase::Up || self.phase == CyclePhase::Refill)),
            is_app_limited: app_limited,
        };

        self.run_congestion_event(&mut evt);
    }

    fn on_congestion_event(
        &mut self,
        _now: Instant,
        _sent: Instant,
        _is_persistent_congestion: bool,
        lost_bytes: u64,
    ) {
        self.pending_lost_bytes += lost_bytes;
        self.bytes_lost_in_round += lost_bytes;
        self.loss_events_in_round += 1;
    }

    fn on_mtu_update(&mut self, new_mtu: u16) {
        self.current_mtu = new_mtu as u64;
        self.min_cwnd = 4 * self.current_mtu;
        self.cwnd = self.cwnd.max(self.min_cwnd);
    }

    fn window(&self) -> u64 {
        self.cwnd
    }

    fn metrics(&self) -> ControllerMetrics {
        ControllerMetrics {
            congestion_window: self.cwnd,
            ssthresh: None,
            pacing_rate: Some(self.pacing_rate * 8),
        }
    }

    fn clone_box(&self) -> Box<dyn Controller> {
        Box::new(self.clone())
    }

    fn initial_window(&self) -> u64 {
        self.config.initial_window
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl Clone for Bbr2 {
    fn clone(&self) -> Self {
        // BandwidthEstimation is not (re)cloned precisely; a fresh estimation
        // window is acceptable for the rarely-used clone path.
        Bbr2 {
            config: self.config.clone(),
            current_mtu: self.current_mtu,
            max_bandwidth: self.max_bandwidth.clone(),
            mss: self.mss,
            min_rtt: self.min_rtt,
            latest_rtt: self.latest_rtt,
            min_rtt_timestamp: self.min_rtt_timestamp,
            mode: self.mode,
            phase: self.phase,
            full_bandwidth_reached: self.full_bandwidth_reached,
            full_bandwidth_baseline: self.full_bandwidth_baseline,
            rounds_without_bandwidth_growth: self.rounds_without_bandwidth_growth,
            inflight_hi: self.inflight_hi,
            inflight_lo: self.inflight_lo,
            bandwidth_lo: self.bandwidth_lo,
            cwnd: self.cwnd,
            init_cwnd: self.init_cwnd,
            min_cwnd: self.min_cwnd,
            cwnd_lo: self.cwnd_lo,
            cwnd_hi: self.cwnd_hi,
            pacing_gain: self.pacing_gain,
            cwnd_gain: self.cwnd_gain,
            pacing_rate: self.pacing_rate,
            acked_bytes: self.acked_bytes,
            bytes_lost_in_round: self.bytes_lost_in_round,
            loss_events_in_round: self.loss_events_in_round,
            min_bytes_in_flight_in_round: self.min_bytes_in_flight_in_round,
            pending_lost_bytes: self.pending_lost_bytes,
            prev_in_flight_count: self.prev_in_flight_count,
            last_sample_is_app_limited: self.last_sample_is_app_limited,
            round_count: self.round_count,
            current_round_trip_end_packet_number: self.current_round_trip_end_packet_number,
            max_sent_packet_number: self.max_sent_packet_number,
            max_acked_packet_number: self.max_acked_packet_number,
            cycle_start_time: self.cycle_start_time,
            phase_start_time: self.phase_start_time,
            rounds_in_phase: self.rounds_in_phase,
            rounds_since_probe: self.rounds_since_probe,
            probe_wait_time: self.probe_wait_time,
            probe_up_rounds: self.probe_up_rounds,
            probe_up_bytes: self.probe_up_bytes,
            probe_up_acked: self.probe_up_acked,
            is_sample_from_probing: self.is_sample_from_probing,
            has_advanced_max_bw: self.has_advanced_max_bw,
            last_cycle_probed_too_high: self.last_cycle_probed_too_high,
            last_cycle_stopped_risky_probe: self.last_cycle_stopped_risky_probe,
            exit_probe_rtt_at: self.exit_probe_rtt_at,
            probe_rtt_last_started_at: self.probe_rtt_last_started_at,
        }
    }
}

fn bw_from_delta(bytes: u64, delta: Duration) -> u64 {
    let ns = delta.as_nanos();
    if ns == 0 {
        return 0;
    }
    (bytes * 1_000_000_000) / (ns as u64)
}

/// Configuration for the [`Bbr2`] congestion controller.
#[derive(Debug, Clone)]
pub struct BbrConfig {
    initial_window: u64,
}

impl BbrConfig {
    /// Set the initial congestion window in bytes.
    pub fn initial_window(&mut self, value: u64) -> &mut Self {
        self.initial_window = value;
        self
    }
}

impl Default for BbrConfig {
    fn default() -> Self {
        Self {
            initial_window: 10 * BASE_DATAGRAM_SIZE,
        }
    }
}

impl ControllerFactory for BbrConfig {
    fn build(self: Arc<Self>, _now: Instant, current_mtu: u16) -> Box<dyn Controller> {
        Box::new(Bbr2::new(self, current_mtu))
    }
}