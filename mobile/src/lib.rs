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
use jni::objects::{JObject, JValue};
use jni::sys::{jint, jlong, jobject};
use jni::JNIEnv;
use tokio::runtime::{Builder, Runtime};
use tokio::task::AbortHandle;

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
/// one line per peer as `"name\tip:port"` (empty string if none found). The
/// 设备 tab renders these lines into [Device] cards. No `JNIEnv` crosses any
/// `await` point — the whole scan is awaited via `runtime().block_on`.
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
        .map(|p| format!("{}\t{}", p.device_name, p.socket_addr()))
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
