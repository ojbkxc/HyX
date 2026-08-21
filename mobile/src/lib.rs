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
use std::time::Instant;

use hyx_core::identity::Identity;
use hyx_core::progress::{ProgressCallback, ProgressState};
use hyx_core::protocol::ConfigMessage;
use hyx_core::reconnect::ReconnectConfig;
use hyx_core::session::P2PSession;
use hyx_core::transfer_folder::AcceptDecision;
use hyx_core::DEFAULT_RENDEZVOUS_PORT;
use jni::objects::{JObject, JValue};
use jni::sys::{jint, jlong, jobject};
use jni::JNIEnv;
use tokio::runtime::{Builder, Runtime};
use tokio::task::AbortHandle;
use uuid::Uuid;

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
    RT.get_or_init(|| Builder::new_multi_thread().enable_all().build().expect("tokio runtime"))
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

/// Fresh per-process device id; used to make rendezvous initiator/responder split deterministic.
static DEVICE_ID: OnceLock<Uuid> = OnceLock::new();
fn device_id() -> Uuid {
    *DEVICE_ID.get_or_init(Uuid::new_v4)
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
    let mut c = ConfigMessage::default();
    c.chunk_size = (chunk_bytes as u32).max(64 * 1024);
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
fn progress_sink(tx: std::sync::mpsc::SyncSender<Evt>) -> ProgressCallback {
    let t0 = Instant::now();
    Box::new(move |done, total| {
        let el = t0.elapsed().as_secs_f64();
        let rate = if el > 0.0 { (done as f64) / el } else { 0.0 };
        let _ = tx.send(Evt::Progress(done, total, rate as i64));
    })
}

/// Receive whatever the peer sends into `dir` after `session` is established.
async fn receive_into(
    session: &mut P2PSession,
    dir: &str,
    tx: std::sync::mpsc::SyncSender<Evt>,
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

/// Send `path` to the peer we already connected to.
async fn send_path(
    session: &mut P2PSession,
    path: &str,
    tx: std::sync::mpsc::SyncSender<Evt>,
) -> Result<String, String> {
    let mut prog = ProgressState::new(0);
    prog.set_progress_callback(progress_sink(tx.clone()));
    session
        .send_path(
            std::path::Path::new(path),
            &ReconnectConfig::default(),
            None,
            Some(&mut prog),
        )
        .await
        .map(|_| String::new()) // "" == success
        .map_err(|e| e.to_string())
}

/// Synchronously drain the event channel, calling the Kotlin callback as events
/// arrive, and return one final `jstring` result (summary or error).
fn drain(mut env: JNIEnv<'_>, cb: JObject<'_>, rx: &std::sync::mpsc::Receiver<Evt>) -> jobject {
    while let Ok(ev) = rx.recv() {
        match ev {
            Evt::Progress(done, total, rate) => {
                let _ = env.call_method(
                    cb,
                    "onProgress",
                    "(IJJJ)V",
                    &[
                        JValue::Int(2),
                        JValue::Long(done as jlong),
                        JValue::Long(total as jlong),
                        JValue::Long(rate),
                    ],
                );
            }
            Evt::Done(res) => {
                return match res {
                    Ok(msg) => new_jstring(&mut env, &msg),
                    Err(err) => new_jstring(&mut env, &err),
                };
            }
        }
    }
    std::ptr::null_mut()
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
/// bind + accept + receive into `saveDir`.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxStartListener<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    port: jint,
    chunk_bytes: jint,
    _fsync_every: jlong,
    compression: jint,
    _aggregation: jint,
    save_dir: jni::objects::JString<'local>,
    cb: JObject<'local>,
) -> jobject {
    let dir = env
        .get_string(&save_dir)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = config_from(chunk_bytes, compression);

    let join = runtime().spawn(async move {
        let mut session = match P2PSession::accept(bind_addr(port), identity(), device_id()).await {
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

/// `String hyxConnect(String peerAddress, String filePath, int chunkBytes,
/// long fsyncEveryBytes, int compression, int aggregation, int port,
/// ProgressCallback cb)` — LAN-discover the peer, connect, send `filePath`.
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
    let _peer = env
        .get_string(&peer_address)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = config_from(chunk_bytes, compression);

    let join = runtime().spawn(async move {
        let (addr, fp) = match P2PSession::discover_one_peer(port as u16, &identity(), device_id()).await
        {
            Ok(pair) => pair,
            Err(e) => {
                let _ = tx.send(Evt::Done(Err(e.to_string())));
                return;
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

    let join = runtime().spawn(async move {
        let rv = match P2PSession::parse_peer_addr(&server, DEFAULT_RENDEZVOUS_PORT) {
            Ok(addr) => addr,
            Err(e) => {
                let _ = tx.send(Evt::Done(Err(e.to_string())));
                return;
            }
        };
        let mut session = match P2PSession::from_rendezvous(
            rv,
            code,
            identity(),
            device_id(),
            cfg,
            false,
        )
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
    env: JNIEnv<'local>,
    _this: JObject<'local>,
) -> jobject {
    if let Some(h) = ACTIVE.lock().expect("active lock").take() {
        h.abort();
    }
    std::ptr::null_mut()
}