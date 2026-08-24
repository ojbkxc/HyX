//! Brutal 拥塞控制 — 固定带宽，丢包不退避。
//!
//! 参考 Hysteria 2 的 Brutal 算法：用户指定最大带宽，发送方按固定速率
//! 发送，丢包不降速。适合"越快越好"的文件传输场景，尤其在高丢包/高延迟
//! 环境下远超标准 Cubic/Reno 的退避行为。
//!
//! 拥塞窗口 = max_bandwidth × RTT（BDP，Bandwidth-Delay Product）。
//! 丢包时窗口不变，QUIC 的重传机制处理丢包恢复。

use std::any::Any;
use std::sync::Arc;

use crate::connection::RttEstimator;
use crate::Instant;

use super::{BASE_DATAGRAM_SIZE, Controller, ControllerFactory, ControllerMetrics};

/// Brutal 拥塞控制器 — 固定带宽，丢包不退避。
pub struct Brutal {
    config: Arc<BrutalConfig>,
    /// 当前 RTT（纳秒）
    rtt_nanos: u64,
    /// 当前 MTU
    mtu: u16,
    /// 拥塞窗口（bytes）= max_bandwidth × RTT
    cwnd: u64,
}

impl Brutal {
    /// Construct a Brutal controller from the given config and current MTU.
    pub fn new(config: Arc<BrutalConfig>, current_mtu: u16) -> Self {
        // 初始 RTT 估计 30ms（局域网典型值）
        let rtt_nanos = 30_000_000;
        let cwnd = Self::compute_window(config.max_bandwidth, rtt_nanos, current_mtu);
        Self {
            config,
            rtt_nanos,
            mtu: current_mtu,
            cwnd,
        }
    }

    /// 计算 BDP 窗口 = bandwidth × RTT，至少 10 个 MTU。
    fn compute_window(max_bandwidth: u64, rtt_nanos: u64, mtu: u16) -> u64 {
        // BDP = max_bandwidth (bytes/sec) × rtt (nanos) / 1_000_000_000
        // 拆成两部分相乘再相加，避免 u64 溢出：先整除部分，再取模部分。
        let bdp = (max_bandwidth / 1_000_000_000) * rtt_nanos
            + (max_bandwidth % 1_000_000_000) * rtt_nanos / 1_000_000_000;
        // 至少 10 个 MTU，避免窗口太小导致 stall
        bdp.max(10 * mtu as u64)
    }
}

impl Controller for Brutal {
    fn on_sent(&mut self, _now: Instant, _bytes: u64, _last_packet_number: u64) {
        // Brutal 不需要跟踪发送量
    }

    fn on_ack(
        &mut self,
        _now: Instant,
        _sent: Instant,
        _bytes: u64,
        _app_limited: bool,
        rtt: &RttEstimator,
    ) {
        // 用最新的 RTT 更新窗口
        let new_rtt = rtt.get().as_nanos() as u64;
        if new_rtt > 0 && new_rtt != self.rtt_nanos {
            self.rtt_nanos = new_rtt;
            self.cwnd = Self::compute_window(self.config.max_bandwidth, self.rtt_nanos, self.mtu);
        }
    }

    fn on_end_acks(
        &mut self,
        _now: Instant,
        _in_flight: u64,
        _app_limited: bool,
        _largest_packet_num_acked: Option<u64>,
    ) {
        // 不需要
    }

    /// Brutal 的核心：丢包不降速！什么都不做！
    fn on_congestion_event(
        &mut self,
        _now: Instant,
        _sent: Instant,
        _is_persistent_congestion: bool,
        _lost_bytes: u64,
    ) {
        // 故意留空 — 这就是 Brutal 的精髓。
        // 丢包不退避，窗口不变，继续按固定速率发送。
    }

    fn on_mtu_update(&mut self, new_mtu: u16) {
        self.mtu = new_mtu;
        self.cwnd = Self::compute_window(self.config.max_bandwidth, self.rtt_nanos, self.mtu);
    }

    fn window(&self) -> u64 {
        self.cwnd
    }

    fn metrics(&self) -> ControllerMetrics {
        ControllerMetrics {
            congestion_window: self.cwnd,
            ssthresh: None,
            pacing_rate: Some(self.config.max_bandwidth * 8),
        }
    }

    fn clone_box(&self) -> Box<dyn Controller> {
        Box::new(Brutal {
            config: self.config.clone(),
            rtt_nanos: self.rtt_nanos,
            mtu: self.mtu,
            cwnd: self.cwnd,
        })
    }

    fn initial_window(&self) -> u64 {
        self.config.initial_window
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Brutal 拥塞控制配置。
///
/// `max_bandwidth` 是用户指定的最大带宽（bytes per second）。
/// 默认 10 Gbps（1.25 GB/s），适合"越快越好"的场景。
#[derive(Debug, Clone)]
pub struct BrutalConfig {
    /// 最大带宽（bytes per second）
    max_bandwidth: u64,
    /// 初始拥塞窗口（bytes）
    initial_window: u64,
}

impl BrutalConfig {
    /// 创建指定最大带宽的 Brutal 配置。
    pub fn new(max_bandwidth: u64) -> Self {
        Self {
            max_bandwidth,
            initial_window: 10 * BASE_DATAGRAM_SIZE,
        }
    }

    /// 设置初始拥塞窗口。
    pub fn initial_window(&mut self, value: u64) -> &mut Self {
        self.initial_window = value;
        self
    }
}

impl Default for BrutalConfig {
    fn default() -> Self {
        Self {
            // 10 Gbps = 1.25 GB/s = 1_250_000_000 bytes/s
            // "越快越好"：设一个很高的值，在大多数网络下不会成为瓶颈。
            // 窗口 = bandwidth × RTT，在 1ms RTT 下窗口 = 1.25 MB，
            // 在 30ms RTT 下窗口 = 37.5 MB，都在合理范围内。
            max_bandwidth: 1_250_000_000,
            initial_window: 10 * BASE_DATAGRAM_SIZE,
        }
    }
}

impl ControllerFactory for BrutalConfig {
    fn build(self: Arc<Self>, _now: Instant, current_mtu: u16) -> Box<dyn Controller> {
        Box::new(Brutal::new(self, current_mtu))
    }
}