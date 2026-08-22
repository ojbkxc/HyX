//! JNI boundary for the Android app — real hyx-core wiring.
//!
//! Each `#[no_mangle] extern "system"` symbol here is the FFI counterpart of an
//! `external fun` in `HyXNative.kt`. The design keeps `JNIEnv` OUT of any
//! `await` point: arguments are read synchronously, the actual session runs on
//! a global Tokio runtime, and progress/result events are pushed back through a
//! std mpsc channel that the JNI thread drains synchronously (calling
//! `onProgress` / building the result `jstring` with a short-lived `JNIEnv`).
//!
//! Call graph per phone:
//!   receiver  <- hyxStartListener  = bind + P2PSession::accept + receive_to
//!   receiver  <- hyxPairRendezvous = rendezvous handshake + receive_to
//!   sender    <- hyxConnect        = LAN discover + P2PSession::connect + send_path

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use hyx_core::discovery::DiscoveryManager;
use hyx_core::identity::{device_id_from_fingerprint, Identity};
use hyx_core::progress::{ProgressCallback, ProgressState};
use hyx_core::protocol::ConfigMessage;
use hyx_core::reconnect::ReconnectConfig;
use hyx_core::session::P2PSession;
use hyx_core::transfer_folder::AcceptDecision;
use hyx_core::Uuid;
use hyx_core::DEFAULT_RENDEZVOUS_PORT;
use jni::objects::{GlobalRef, JObject, JValue};
use jni::sys::{jint, jlong, jobject};
use jni::{JNIEnv, JavaVM};
use tokio::runtime::{Builder, Runtime};
use tokio::task::AbortHandle;
use tracing_subscriber::field::Visit;
use tracing_subscriber::layer::{Context, Layer};

/// Event a JNI call waits on from the background transfer task.
enum Evt {
    /// Phase 2 always; `(bytes_done, bytes_total, speed_bps)`.
    Progress(u64, u64, i64),
    /// Transfer finished: `Ok(summary)` or `Err(message)`.
    Done(Result<String, String>),
}

/// One global Tokio runtime shared by every JNI call.
static RT: OnceLock<Runtime> = OnceLock::new();
fn runtime() -> &'static Runtime {
    RT.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Long-lived device identity (generated once, persisted by hyx-core).
static IDENTITY: OnceLock<Arc<Identity>> = OnceLock::new();
fn identity() -> Arc<Identity> {
    IDENTITY
        .get_or_init(|| {
            Arc::new(
                Identity::load_or_generate(None)
                    .unwrap_or_else(|_| Identity::generate().expect("generate identity")),
            )
        })
        .clone()
}

/// Stable per-device id derived from the cert fingerprint, so a peer's
/// identity survives app restarts. Used to make rendezvous initiator/
/// responder split deterministic and to identify us in discovery beacons.
static DEVICE_ID: OnceLock<Uuid> = OnceLock::new();
fn device_id() -> Uuid {
    *DEVICE_ID.get_or_init(|| device_id_from_fingerprint(&identity().fingerprint()))
}

/// Handle of the in-flight background transfer, so `hyxCancel` can abort it.
static ACTIVE: Mutex<Option<AbortHandle>> = Mutex::new(None);

fn track(handle: AbortHandle) {
    *ACTIVE.lock().expect("active lock") = Some(handle);
}
fn forget_active() {
    *ACTIVE.lock().expect("active lock") = None;
}

/// Map the FFI `compression` knob (0=off, 1=adaptive, 2=always) onto ConfigMessage.
fn config_from(chunk_bytes: jint, compression: jint) -> ConfigMessage {
    let mut c = ConfigMessage {
        chunk_size: (chunk_bytes as u32).max(64 * 1024),
        ..ConfigMessage::default()
    };
    match compression {
        0 => c.compression_enabled = false,
        2 => {
            c.compression_enabled = true;
            c.adaptive_compression = false;
        }
        _ => {
            c.compression_enabled = true;
            c.adaptive_compression = true;
        }
    }
    c
}

/// Progress callback that mirrors bytes into the JNI event channel.
///
/// Throttled to ~5 Hz with a sliding-window rate so the Android UI isn't
/// flooded with one callback per chunk and the shown speed/ETA reflects the
/// recent pace rather than a lifetime average. `ProgressCallback` is `Fn`, so
/// the throttle state sits behind a mutex captured by the closure.
fn progress_sink(tx: std::sync::mpsc::Sender<Evt>) -> ProgressCallback {
    struct Throttle {
        last_emit: Instant,
        last_done: u64,
    }
    let throttle = std::sync::Arc::new(std::sync::Mutex::new(Throttle {
        last_emit: Instant::now(),
        last_done: 0,
    }));
    Box::new(move |done, total| {
        let mut st = throttle.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let el = now.duration_since(st.last_emit);
        if el < Duration::from_millis(200) {
            return;
        }
        let rate = if el.as_secs_f64() > 0.0 {
            (done.saturating_sub(st.last_done)) as f64 / el.as_secs_f64()
        } else {
            0.0
        };
        st.last_emit = now;
        st.last_done = done;
        drop(st);
        let _ = tx.send(Evt::Progress(done, total, rate as i64));
    })
}

/// Receive whatever the peer sends into `dir` after `session` is established.
async fn receive_into(
    session: &mut P2PSession,
    dir: &str,
    tx: std::sync::mpsc::Sender<Evt>,
) -> Result<String, String> {
    let out = PathBuf::from(dir);
    let mut prog = ProgressState::new(0);
    prog.set_progress_callback(progress_sink(tx.clone()));
    session
        .receive_to(&out, None, |_| AcceptDecision::Accept, Some(&mut prog))
        .await
        .map(|_| String::new()) // "" == success; JNI Caller treats non-empty as failure
        .map_err(|e| e.to_string())
}

/// Send `path` to the peer we already connected to, with on-disk resume
/// state so `send_path`'s internal reconnect/retry loop continues instead of
/// restarting. The state file lives as a sibling of the source; on Android the
/// source sits in the app's cache (writable), and the core deletes the state
/// on success. A transient failure leaves it behind, so the next attempt
/// (same path, same peer output dir) resumes from the completed chunks.
async fn send_path(
    session: &mut P2PSession,
    path: &str,
    tx: std::sync::mpsc::Sender<Evt>,
) -> Result<String, String> {
    let mut prog = ProgressState::new(0);
    prog.set_progress_callback(progress_sink(tx.clone()));

    let src = std::path::Path::new(path);
    let mut state_path = src.to_path_buf();
    if let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) {
        state_path.set_file_name(format!(".{name}.hyx-resume"));
    }
    let state_path = (src.parent().is_some()).then_some(state_path);

    session
        .send_path(
            src,
            &ReconnectConfig::default(),
            state_path.as_deref(),
            Some(&mut prog),
        )
        .await
        .map(|_| String::new()) // "" == success
        .map_err(|e| e.to_string())
}

/// Real LAN UDP-beacon discovery: broadcast + listen for ~2.5 s, then return
/// one line per peer as `"name\tip:port\tdevice_id"` (empty string if none
/// found). The 设备 tab renders these lines into [Device] cards, keyed by the
/// stable `device_id` so the same phone seen on different subnets dedupes.
/// No `JNIEnv` crosses any `await` point — the whole scan is awaited via
/// `runtime().block_on`.
async fn discover_peers(port: u16) -> String {
    let name = format!("hyx-{}", &device_id().to_string()[..6]);
    let manager = match DiscoveryManager::new(
        name,
        port,
        identity().fingerprint(),
        device_id(),
        Duration::from_secs(60),
    )
    .await
    {
        Ok(m) => Arc::new(m),
        Err(e) => {
            tracing::warn!("discovery manager failed: {e}");
            return String::new();
        }
    };
    if let Err(e) = manager.start().await {
        tracing::warn!("discovery start failed: {e}");
        return String::new();
    }
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let peers = manager.get_peers().await;
    manager.stop();
    peers
        .into_iter()
        .map(|p| format!("{}\t{}\t{}", p.device_name, p.socket_addr(), p.device_id))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Synchronously drain the event channel, calling the Kotlin callback as events
/// arrive, and return one final `jstring` result (summary or error).
fn drain(mut env: JNIEnv<'_>, cb: JObject<'_>, rx: &std::sync::mpsc::Receiver<Evt>) -> jobject {
    while let Ok(ev) = rx.recv() {
        match ev {
            Evt::Progress(done, total, rate) => {
                let _ = env.call_method(
                    &cb,
                    "onProgress",
                    "(IJJJ)V",
                    &[
                        JValue::Int(2),
                        JValue::Long(done as jlong),
                        JValue::Long(total as jlong),
                        JValue::Long(rate),
                    ],
                );
                // Clear any exception thrown by the Kotlin callback so
                // subsequent JNI calls (more progress events, the final
                // jstring) don't silently fail with a pending exception.
                let _ = env.exception_clear();
            }
            Evt::Done(res) => {
                return match res {
                    Ok(msg) => new_jstring(&mut env, &msg),
                    Err(err) => new_jstring(&mut env, &err),
                };
            }
        }
    }
    // All senders dropped without a Done event: the transfer was aborted
    // (hyxCancel) or the task panicked. Return a non-empty error so the
    // Kotlin side treats it as a failure instead of a silent success.
    new_jstring(&mut env, "Transfer cancelled")
}

fn new_jstring(env: &mut JNIEnv<'_>, s: &str) -> jobject {
    match env.new_string(s) {
        Ok(j) => j.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[inline]
fn bind_addr(port: jint) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port as u16)
}

// ---------------------------------------------------------------------------
// Rust tracing → JNI log callback
// ---------------------------------------------------------------------------
//
// `tracing::event!` calls (e.g. the `tracing::warn!` in `discover_peers` and
// `hyxStartListener`) are forwarded to Kotlin through a global `LogCallback`
// registered via `hyxSetLogCallback`. The path is:
//
//   tracing::event!
//     → JniLogLayer::on_event           (tracing-subscriber Layer)
//     → attach_current_thread()         (JNIEnv for this thread)
//     → callback.onLog(level, tag, msg) (JNI call_method)
//     → LogCollector.add(LogEntry)      (Kotlin side)
//
// `JavaVM` + `GlobalRef` are `Send + Sync` and storable in `OnceLock`; we never
// hold a `JNIEnv` across threads — each event re-attaches (a no-op if already
// attached) and clears any pending exception so the next event isn't blocked.

/// Global JavaVM reference, set once in `hyxSetLogCallback`. Tracing events fire
/// on arbitrary Tokio worker threads, so we keep the `JavaVM` (not a `JNIEnv`)
/// and `attach_current_thread()` per event.
static JVM: OnceLock<JavaVM> = OnceLock::new();

/// Global log callback reference (Kotlin `LogCallback` wrapped in `GlobalRef`).
/// `GlobalRef` survives GC and is `Send + Sync`, safe to invoke from any thread
/// after attaching. Held for the process lifetime — the log callback is unique
/// and never replaced.
static LOG_CB: OnceLock<GlobalRef> = OnceLock::new();

/// `tracing-subscriber` Layer that forwards every event to Kotlin via JNI.
struct JniLogLayer;

impl<S: tracing_core::Subscriber> Layer<S> for JniLogLayer {
    fn on_event(&self, event: &tracing_core::Event<'_>, _ctx: Context<'_, S>) {
        let Some(vm) = JVM.get() else { return };
        let Some(cb) = LOG_CB.get() else { return };

        let level = level_to_int(event.metadata().level());
        let target = event.metadata().target();

        // Collect the "message" field plus any extras as `key=value`.
        let mut visitor = FieldCollector::default();
        event.record(&mut visitor);
        let message = visitor.finish();

        // `attach_current_thread` is a no-op when the thread is already
        // attached, so calling it on every event is cheap and safe.
        let Ok(mut env) = vm.attach_current_thread() else {
            return;
        };
        let Ok(jtag) = env.new_string(target) else {
            return;
        };
        let Ok(jmsg) = env.new_string(&message) else {
            return;
        };
        let _ = env.call_method(
            cb.as_obj(),
            "onLog",
            "(ILjava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Int(level),
                JValue::Object(&jtag),
                JValue::Object(&jmsg),
            ],
        );
        // Clear any exception thrown by the Kotlin callback so subsequent JNI
        // calls (more log events) don't silently fail with a pending exception.
        let _ = env.exception_clear();
    }
}

/// Map a `tracing` `Level` to the int ordinal expected by Kotlin's
/// `LogLevel` enum (0=TRACE, 1=DEBUG, 2=INFO, 3=WARN, 4=ERROR).
fn level_to_int(l: &tracing_core::Level) -> jint {
    match *l {
        tracing_core::Level::TRACE => 0,
        tracing_core::Level::DEBUG => 1,
        tracing_core::Level::INFO => 2,
        tracing_core::Level::WARN => 3,
        tracing_core::Level::ERROR => 4,
    }
}

/// `tracing` field visitor that collects the conventional `"message"` field
/// verbatim and formats every other field as `name=value`. Used by
/// `JniLogLayer::on_event` to reconstruct a single human-readable string from
/// the event's structured fields.
#[derive(Default)]
struct FieldCollector {
    message: String,
    extras: Vec<String>,
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &tracing_core::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.extras.push(format!("{}={:?}", field.name(), value));
        }
    }

    // Explicit `record_str` avoids the Debug quoting (`"..."`) that
    // `record_debug` would add for string fields, keeping messages clean.
    fn record_str(&mut self, field: &tracing_core::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.extras.push(format!("{}={}", field.name(), value));
        }
    }
}

impl FieldCollector {
    /// Join the collected message and extras into a single string. The message
    /// (if any) comes first, followed by `key=value` pairs separated by spaces.
    fn finish(mut self) -> String {
        if self.extras.is_empty() {
            self.message
        } else if self.message.is_empty() {
            self.extras.join(" ")
        } else {
            self.extras.insert(0, self.message);
            self.extras.join(" ")
        }
    }
}

// ---------------------------------------------------------------------------
// FFI entry points (symbols must match HyXNative.kt method names).
// ---------------------------------------------------------------------------

/// `String hyxCreateDevice(String ignored)` — load/generate identity,
/// return its hex cert fingerprint (or null on failure).
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxCreateDevice<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    _self_cert: jni::objects::JString<'local>,
) -> jobject {
    match identity().fingerprint_hex() {
        s if !s.is_empty() => new_jstring(&mut env, &s),
        _ => std::ptr::null_mut(),
    }
}

/// `String hyxStartListener(int port, int chunkBytes, long fsyncEveryBytes,
/// int compression, int aggregation, String saveDir, ProgressCallback cb)` —
/// bind + accept + receive into `saveDir`. While listening, also broadcasts
/// LAN beacons so a sender's discovery scan can find this device.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxStartListener<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    port: jint,
    _chunk_bytes: jint,
    _fsync_every: jlong,
    _compression: jint,
    _aggregation: jint,
    save_dir: jni::objects::JString<'local>,
    cb: JObject<'local>,
) -> jobject {
    let dir = env
        .get_string(&save_dir)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = std::sync::mpsc::channel();

    let join = runtime().spawn(async move {
        // Broadcast beacons for the whole listen window so senders can
        // discover us. Best-effort: a bind failure (e.g. another instance
        // already owns the discovery port) must not block receiving.
        let discovery = Arc::new(
            DiscoveryManager::new(
                format!("hyx-{}", &device_id().to_string()[..6]),
                port as u16,
                identity().fingerprint(),
                device_id(),
                Duration::from_secs(60),
            )
            .await
            .ok(),
        );
        if let Some(d) = discovery.as_ref() {
            if let Err(e) = d.start().await {
                tracing::warn!("discovery start failed: {e}");
            }
        }

        let mut session = match P2PSession::accept(bind_addr(port), identity(), device_id()).await {
            Ok(s) => s,
            Err(e) => {
                if let Some(d) = discovery.as_ref() {
                    d.stop();
                }
                let _ = tx.send(Evt::Done(Err(e.to_string())));
                return;
            }
        };
        let res = receive_into(&mut session, &dir, tx.clone()).await;
        if let Some(d) = discovery.as_ref() {
            d.stop();
        }
        let _ = tx.send(Evt::Done(res));
    });
    track(join.abort_handle());

    let out = drain(env, cb, &rx);
    forget_active();
    out
}

/// `String hyxConnect(String peerAddress, String filePath, int chunkBytes,
/// long fsyncEveryBytes, int compression, int aggregation, int port,
/// ProgressCallback cb)` — connect to `peerAddress` (LAN-discovering it for
/// the cert fingerprint when the address is empty), then send `filePath`.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxConnect<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    peer_address: jni::objects::JString<'local>,
    file_path: jni::objects::JString<'local>,
    chunk_bytes: jint,
    _fsync_every: jlong,
    compression: jint,
    _aggregation: jint,
    port: jint,
    cb: JObject<'local>,
) -> jobject {
    let path = env
        .get_string(&file_path)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    let peer = env
        .get_string(&peer_address)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = config_from(chunk_bytes, compression);

    let join = runtime().spawn(async move {
        // A specific address (from the 设备 tab) wins; otherwise fall back
        // to discovering whatever peer is on the LAN. Either way we get the
        // peer's cert fingerprint from its beacon so TLS can pin it.
        let (addr, fp) = if !peer.is_empty() {
            let target = match P2PSession::resolve_peer_addr(&peer, port as u16).await {
                Ok(a) => a,
                Err(e) => {
                    let _ = tx.send(Evt::Done(Err(e.to_string())));
                    return;
                }
            };
            match P2PSession::discover_peer(port as u16, &identity(), device_id(), Some(target))
                .await
            {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = tx.send(Evt::Done(Err(e.to_string())));
                    return;
                }
            }
        } else {
            match P2PSession::discover_one_peer(port as u16, &identity(), device_id()).await {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = tx.send(Evt::Done(Err(e.to_string())));
                    return;
                }
            }
        };
        let mut session = match P2PSession::connect(addr, fp, identity(), device_id(), cfg).await {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(Evt::Done(Err(e.to_string())));
                return;
            }
        };
        let res = send_path(&mut session, &path, tx.clone()).await;
        let _ = tx.send(Evt::Done(res));
    });
    track(join.abort_handle());

    let out = drain(env, cb, &rx);
    forget_active();
    out
}

/// `String hyxPairRendezvous(String code, String serverAddress, int port,
/// int compression, String saveDir, ProgressCallback cb)` — rendezvous
/// handshake, then act as receiver into `saveDir`.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxPairRendezvous<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    code: jni::objects::JString<'local>,
    server: jni::objects::JString<'local>,
    port: jint,
    compression: jint,
    save_dir: jni::objects::JString<'local>,
    cb: JObject<'local>,
) -> jobject {
    let code = env
        .get_string(&code)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    let server = env
        .get_string(&server)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = env
        .get_string(&save_dir)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = config_from(1024 * 1024, compression);

    let join =
        runtime().spawn(async move {
            let rv = match P2PSession::resolve_peer_addr(
                &server,
                if port > 0 {
                    port as u16
                } else {
                    DEFAULT_RENDEZVOUS_PORT
                },
            )
            .await
            {
                Ok(addr) => addr,
                Err(e) => {
                    let _ = tx.send(Evt::Done(Err(e.to_string())));
                    return;
                }
            };
            let mut session =
                match P2PSession::from_rendezvous(rv, code, identity(), device_id(), cfg, false)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(Evt::Done(Err(e.to_string())));
                        return;
                    }
                };
            let res = receive_into(&mut session, &dir, tx.clone()).await;
            let _ = tx.send(Evt::Done(res));
        });
    track(join.abort_handle());

    let out = drain(env, cb, &rx);
    forget_active();
    out
}

/// `String hyxPairSend(String code, String serverAddress, int port,
/// String filePath, int chunkBytes, int compression, int aggregation,
/// ProgressCallback cb)` — rendezvous handshake, then send [filePath] to the
/// paired peer. The sender side of a code/QR share: generate a code, show it
/// as a QR (or let the peer type it), and this call blocks on `from_rendezvous`
/// until the peer dials in with the same code — then streams the file out.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxPairSend<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    code: jni::objects::JString<'local>,
    server: jni::objects::JString<'local>,
    port: jint,
    file_path: jni::objects::JString<'local>,
    chunk_bytes: jint,
    compression: jint,
    _aggregation: jint,
    cb: JObject<'local>,
) -> jobject {
    let code = env
        .get_string(&code)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    let server = env
        .get_string(&server)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = env
        .get_string(&file_path)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = config_from(chunk_bytes, compression);

    let join =
        runtime().spawn(async move {
            let rv = match P2PSession::resolve_peer_addr(
                &server,
                if port > 0 {
                    port as u16
                } else {
                    DEFAULT_RENDEZVOUS_PORT
                },
            )
            .await
            {
                Ok(addr) => addr,
                Err(e) => {
                    let _ = tx.send(Evt::Done(Err(e.to_string())));
                    return;
                }
            };
            let mut session =
                match P2PSession::from_rendezvous(rv, code, identity(), device_id(), cfg, false)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(Evt::Done(Err(e.to_string())));
                        return;
                    }
                };
            let res = send_path(&mut session, &path, tx.clone()).await;
            let _ = tx.send(Evt::Done(res));
        });
    track(join.abort_handle());

    let out = drain(env, cb, &rx);
    forget_active();
    out
}

/// `String hyxCancel()` — abort the in-flight transfer.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxCancel<'local>(
    _env: JNIEnv<'local>,
    _this: JObject<'local>,
) -> jobject {
    if let Some(h) = ACTIVE.lock().expect("active lock").take() {
        h.abort();
    }
    std::ptr::null_mut()
}

/// `String hyxDiscover(int port)` — real LAN discovery. Returns newline-joined
/// `"name\tip:port"` lines ("" if none found).
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxDiscover<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    port: jint,
) -> jobject {
    let result = runtime().block_on(discover_peers(if port > 0 { port as u16 } else { 14567 }));
    new_jstring(&mut env, &result)
}

/// `void hyxSetLogCallback(LogCallback cb)` — register the global Rust→Android
/// log callback and install the global `tracing` subscriber.
///
/// Must be called once from `HyXApp.onCreate` after `ensureLoaded()` (so
/// `libhyx_mobile.so` is mapped) and before any other JNI call that may emit a
/// `tracing` event. Subsequent calls are no-ops: `OnceLock::set` ignores
/// repeated writes, and `set_global_default` returns an error if a global
/// subscriber is already installed (which we silently drop).
///
/// After this returns, every `tracing::event!` on any thread (including Tokio
/// workers) is forwarded to `cb.onLog(level, tag, message)` via JNI.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxSetLogCallback<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    callback: JObject<'local>,
) {
    // 1. Obtain the JavaVM — needed to attach arbitrary worker threads later.
    let vm = match env.get_java_vm() {
        Ok(vm) => vm,
        Err(_) => return,
    };

    // 2. Promote the local callback ref to a global ref so it outlives this
    //    JNI frame and GC. `GlobalRef` is `Send + Sync`.
    let global = match env.new_global_ref(callback) {
        Ok(g) => g,
        Err(_) => return,
    };

    // 3. Stash both in process-global `OnceLock`s. Repeated calls (e.g. after
    //    an Activity recreation) silently keep the first registration.
    let _ = JVM.set(vm);
    let _ = LOG_CB.set(global);

    // 4. Install the global tracing subscriber. `set_global_default` errors if
    //    one already exists — we ignore that, matching the "first call wins"
    //    semantics of the `OnceLock`s above.
    let subscriber = tracing_subscriber::registry().with(JniLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 5. Bridge the `log` crate onto `tracing` so any `log::info!`/`log::warn!`
    //    inside hyx-core (or dependencies) is forwarded to `JniLogLayer` too.
    //    `LogTracer::init` sets `log`'s global logger; failure (already set) is
    //    ignored for the same reason as above.
    let _ = tracing_log::LogTracer::init();
}
