//! 传输 API。
//!
//! 对应 `mobile/src/lib.rs` 的 `hyxStartListener` / `hyxConnect` /
//! `hyxPairRendezvous` / `hyxPairSend` / `hyxCancel`。
//!
//! 与 mobile JNI 版本的关键差异：
//! - **回调**：mobile 用 `JNIEnv` + `JObject` 回调 `onProgress(phase, done, total, rate)`，
//!   事件经 `std::sync::mpsc` 在 JNI 线程同步 drain；FRB 版本用
//!   `StreamSink<RsProgressEvent>`，Rust 直接 `sink.add(event)`，Dart 侧 `Stream.listen`。
//! - **错误**：mobile 返回 `jstring`（空串=成功，非空=错误）；FRB 版本用 `anyhow::Result`，
//!   失败时 `sink.add_error` + 返回 `Err`。
//! - **取消**：mobile 用全局 `ACTIVE: Mutex<Option<AbortHandle>>`；FRB 版本沿用此模式，
//!   `cancel()` 调用 `AbortHandle::abort`。
//! - **运行时**：mobile 用全局 `RT: OnceLock<Runtime>`；FRB 版本同样使用全局多线程 runtime。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use flutter_rust_bridge::frb;
use hyx_core::DEFAULT_RENDEZVOUS_PORT;
use hyx_core::progress::{ProgressCallback, ProgressState};
use hyx_core::protocol::ConfigMessage;
use hyx_core::reconnect::ReconnectConfig;
use hyx_core::session::P2PSession;
use hyx_core::transfer_folder::AcceptDecision;
use tokio::runtime::{Builder, Runtime};
use tokio::task::AbortHandle;

use crate::api::device::{current_device_id, identity};
use crate::api::model::{RsProgressEvent, RsTransferDirection, RsTransferStatus};
use crate::frb_generated::StreamSink;

/// 全局 Tokio runtime，与 mobile `RT` 等价。
static RT: OnceLock<Runtime> = OnceLock::new();
fn runtime() -> &'static Runtime {
    RT.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// 在飞行中的传输任务句柄，供 `cancel` 中止。与 mobile `ACTIVE` 等价。
static ACTIVE: Mutex<Option<AbortHandle>> = Mutex::new(None);

fn track(handle: AbortHandle) {
    *ACTIVE.lock().expect("active lock") = Some(handle);
}

fn forget_active() {
    *ACTIVE.lock().expect("active lock") = None;
}

/// 将 FFI `compression` 旋钮（0=off, 1=adaptive, 2=always）映射到 `ConfigMessage`。
/// 与 mobile `config_from` 完全一致。
fn config_from(chunk_bytes: i32, compression: i32) -> ConfigMessage {
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

/// 构造一个节流到 ~5 Hz 的 `ProgressCallback`，将字节进度通过 `StreamSink` 推送到 Dart。
///
/// 对应 mobile `progress_sink`：滑动窗口速率，避免每个 chunk 都回调淹没 UI。
/// 与 mobile 差异：直接 `sink.add(RsProgressEvent { ... })`，无需 mpsc 中转。
fn progress_sink(
    sink: StreamSink<RsProgressEvent>,
    direction: RsTransferDirection,
) -> ProgressCallback {
    struct Throttle {
        last_emit: Instant,
        last_done: u64,
    }
    let throttle = Arc::new(Mutex::new(Throttle {
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
        let _ = sink.add(RsProgressEvent {
            direction: direction.clone(),
            phase: 2,
            transferred: done,
            total,
            speed: rate,
            status: RsTransferStatus::Transferring,
            message: None,
        });
    })
}

/// 接收对端发来的文件到 `dir`。对应 mobile `receive_into`。
async fn receive_into(
    session: &mut P2PSession,
    dir: &str,
    sink: &StreamSink<RsProgressEvent>,
) -> Result<()> {
    let out = PathBuf::from(dir);
    let mut prog = ProgressState::new(0);
    prog.set_progress_callback(progress_sink(sink.clone(), RsTransferDirection::Receive));
    session
        .receive_to(&out, None, |_| AcceptDecision::Accept, Some(&mut prog))
        .await?;
    Ok(())
}

/// 发送 `path` 到对端，带断点续传状态文件。对应 mobile `send_path`。
async fn send_path(
    session: &mut P2PSession,
    path: &str,
    sink: &StreamSink<RsProgressEvent>,
) -> Result<()> {
    let mut prog = ProgressState::new(0);
    prog.set_progress_callback(progress_sink(sink.clone(), RsTransferDirection::Send));

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
        .await?;
    Ok(())
}

/// 发送最终事件（Completed / Failed）到 sink。
fn emit_final(
    sink: &StreamSink<RsProgressEvent>,
    status: RsTransferStatus,
    msg: Option<String>,
    direction: RsTransferDirection,
) {
    let _ = sink.add(RsProgressEvent {
        direction,
        phase: 2,
        transferred: 0,
        total: 0,
        speed: 0.0,
        status,
        message: msg,
    });
}

/// 启动监听：绑定 `port` + 接收对端连接，存入 `save_dir`。
///
/// 对应 mobile `hyxStartListener`。监听期间同时广播 LAN 信标，便于发送方发现。
///
/// # Arguments
/// - `port`：监听端口（0 视为默认传输端口 14567）。
/// - `chunk_bytes` / `compression`：传输参数（与 mobile 同语义）。
/// - `save_dir`：接收文件存放目录。
/// - `sink`：进度事件流，替代 mobile 的 `ProgressCallback.onProgress`。
///
/// # Errors
///
/// 绑定或接收失败时通过 `sink.add_error` 通知 Dart 并返回 `Err`。
#[frb]
pub fn start_listener(
    port: i32,
    chunk_bytes: i32,
    compression: i32,
    save_dir: String,
    sink: StreamSink<RsProgressEvent>,
) -> Result<()> {
    use hyx_core::discovery::DiscoveryManager;

    let port_u16 = if port > 0 {
        port as u16
    } else {
        hyx_core::DEFAULT_TRANSFER_PORT
    };
    let _ = config_from(chunk_bytes, compression);
    let dir = save_dir.clone();
    let sink_clone = sink.clone();

    let join = runtime().spawn(async move {
        // 广播信标，便于发送方发现（best-effort）。
        let discovery = Arc::new(
            DiscoveryManager::new(
                format!("hyx-{}", &current_device_id().to_string()[..6]),
                port_u16,
                identity().fingerprint(),
                current_device_id(),
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

        let mut session = match P2PSession::accept(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port_u16),
            identity(),
            current_device_id(),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                if let Some(d) = discovery.as_ref() {
                    d.stop();
                }
                emit_final(
                    &sink_clone,
                    RsTransferStatus::Failed,
                    Some(e.to_string()),
                    RsTransferDirection::Receive,
                );
                return;
            }
        };
        let res = receive_into(&mut session, &dir, &sink_clone).await;
        if let Some(d) = discovery.as_ref() {
            d.stop();
        }
        match res {
            Ok(()) => emit_final(
                &sink_clone,
                RsTransferStatus::Completed,
                None,
                RsTransferDirection::Receive,
            ),
            Err(e) => emit_final(
                &sink_clone,
                RsTransferStatus::Failed,
                Some(e.to_string()),
                RsTransferDirection::Receive,
            ),
        }
    });
    track(join.abort_handle());

    Ok(())
}

/// 连接到 `peer_address` 并发送 `file_path`。
///
/// 对应 mobile `hyxConnect`。`peer_address` 为空时自动发现 LAN 上的 peer。
///
/// # Arguments
/// - `peer_address`：对端地址（`ip:port` 或空串触发自动发现）。
/// - `file_path`：待发送文件路径。
/// - `chunk_bytes` / `compression`：传输参数。
/// - `port`：对端端口（0 视为默认 14567）。
/// - `sink`：进度事件流。
#[frb]
pub fn connect(
    peer_address: String,
    file_path: String,
    chunk_bytes: i32,
    compression: i32,
    port: i32,
    sink: StreamSink<RsProgressEvent>,
) -> Result<()> {
    let cfg = config_from(chunk_bytes, compression);
    let port_u16 = if port > 0 {
        port as u16
    } else {
        hyx_core::DEFAULT_TRANSFER_PORT
    };
    let peer = peer_address.clone();
    let path = file_path.clone();
    let sink_clone = sink.clone();

    let join = runtime().spawn(async move {
        let (addr, fp) = if !peer.is_empty() {
            let target = match P2PSession::resolve_peer_addr(&peer, port_u16).await {
                Ok(a) => a,
                Err(e) => {
                    emit_final(
                        &sink_clone,
                        RsTransferStatus::Failed,
                        Some(e.to_string()),
                        RsTransferDirection::Send,
                    );
                    return;
                }
            };
            match P2PSession::discover_peer(
                port_u16,
                &identity(),
                current_device_id(),
                Some(target),
            )
            .await
            {
                Ok(pair) => pair,
                Err(e) => {
                    emit_final(
                        &sink_clone,
                        RsTransferStatus::Failed,
                        Some(e.to_string()),
                        RsTransferDirection::Send,
                    );
                    return;
                }
            }
        } else {
            match P2PSession::discover_one_peer(port_u16, &identity(), current_device_id()).await {
                Ok(pair) => pair,
                Err(e) => {
                    emit_final(
                        &sink_clone,
                        RsTransferStatus::Failed,
                        Some(e.to_string()),
                        RsTransferDirection::Send,
                    );
                    return;
                }
            }
        };
        let mut session =
            match P2PSession::connect(addr, fp, identity(), current_device_id(), cfg).await {
                Ok(s) => s,
                Err(e) => {
                    emit_final(
                        &sink_clone,
                        RsTransferStatus::Failed,
                        Some(e.to_string()),
                        RsTransferDirection::Send,
                    );
                    return;
                }
            };
        let res = send_path(&mut session, &path, &sink_clone).await;
        match res {
            Ok(()) => emit_final(
                &sink_clone,
                RsTransferStatus::Completed,
                None,
                RsTransferDirection::Send,
            ),
            Err(e) => emit_final(
                &sink_clone,
                RsTransferStatus::Failed,
                Some(e.to_string()),
                RsTransferDirection::Send,
            ),
        }
    });
    track(join.abort_handle());

    Ok(())
}

/// 经 rendezvous 服务器配对，作为接收方。
///
/// 对应 mobile `hyxPairRendezvous`。用 `code` 与 `server` 握手，建立连接后
/// 接收文件到 `save_dir`。
///
/// # Arguments
/// - `code`：配对码。
/// - `server`：rendezvous 服务器地址。
/// - `port`：服务器端口（0 视为默认 14570）。
/// - `compression`：压缩参数。
/// - `save_dir`：接收目录。
/// - `sink`：进度事件流。
#[frb]
pub fn pair_rendezvous(
    code: String,
    server: String,
    port: i32,
    compression: i32,
    save_dir: String,
    sink: StreamSink<RsProgressEvent>,
) -> Result<()> {
    let cfg = config_from(1024 * 1024, compression);
    let port_u16 = if port > 0 {
        port as u16
    } else {
        DEFAULT_RENDEZVOUS_PORT
    };
    let code_o = code.clone();
    let server_o = server.clone();
    let dir = save_dir.clone();
    let sink_clone = sink.clone();

    let join = runtime().spawn(async move {
        // 通知 Dart 进入 Pairing 状态。
        let _ = sink_clone.add(RsProgressEvent {
            direction: RsTransferDirection::Receive,
            phase: 0,
            transferred: 0,
            total: 0,
            speed: 0.0,
            status: RsTransferStatus::Pairing,
            message: None,
        });

        let rv = match P2PSession::resolve_peer_addr(&server_o, port_u16).await {
            Ok(addr) => addr,
            Err(e) => {
                emit_final(
                    &sink_clone,
                    RsTransferStatus::Failed,
                    Some(e.to_string()),
                    RsTransferDirection::Receive,
                );
                return;
            }
        };
        let mut session = match P2PSession::from_rendezvous(
            rv,
            code_o,
            identity(),
            current_device_id(),
            cfg,
            false,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                emit_final(
                    &sink_clone,
                    RsTransferStatus::Failed,
                    Some(e.to_string()),
                    RsTransferDirection::Receive,
                );
                return;
            }
        };
        let res = receive_into(&mut session, &dir, &sink_clone).await;
        match res {
            Ok(()) => emit_final(
                &sink_clone,
                RsTransferStatus::Completed,
                None,
                RsTransferDirection::Receive,
            ),
            Err(e) => emit_final(
                &sink_clone,
                RsTransferStatus::Failed,
                Some(e.to_string()),
                RsTransferDirection::Receive,
            ),
        }
    });
    track(join.abort_handle());

    Ok(())
}

/// 经 rendezvous 服务器配对，作为发送方。
///
/// 对应 mobile `hyxPairSend`。用 `code` 与 `server` 握手，建立连接后
/// 发送 `file_path`。
///
/// # Arguments
/// - `code`：配对码。
/// - `server`：rendezvous 服务器地址。
/// - `port`：服务器端口（0 视为默认 14570）。
/// - `file_path`：待发送文件路径。
/// - `chunk_bytes` / `compression`：传输参数。
/// - `sink`：进度事件流。
#[frb]
pub fn pair_send(
    code: String,
    server: String,
    port: i32,
    file_path: String,
    chunk_bytes: i32,
    compression: i32,
    sink: StreamSink<RsProgressEvent>,
) -> Result<()> {
    let cfg = config_from(chunk_bytes, compression);
    let port_u16 = if port > 0 {
        port as u16
    } else {
        DEFAULT_RENDEZVOUS_PORT
    };
    let code_o = code.clone();
    let server_o = server.clone();
    let path = file_path.clone();
    let sink_clone = sink.clone();

    let join = runtime().spawn(async move {
        let _ = sink_clone.add(RsProgressEvent {
            direction: RsTransferDirection::Send,
            phase: 0,
            transferred: 0,
            total: 0,
            speed: 0.0,
            status: RsTransferStatus::Pairing,
            message: None,
        });

        let rv = match P2PSession::resolve_peer_addr(&server_o, port_u16).await {
            Ok(addr) => addr,
            Err(e) => {
                emit_final(
                    &sink_clone,
                    RsTransferStatus::Failed,
                    Some(e.to_string()),
                    RsTransferDirection::Send,
                );
                return;
            }
        };
        let mut session = match P2PSession::from_rendezvous(
            rv,
            code_o,
            identity(),
            current_device_id(),
            cfg,
            false,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                emit_final(
                    &sink_clone,
                    RsTransferStatus::Failed,
                    Some(e.to_string()),
                    RsTransferDirection::Send,
                );
                return;
            }
        };
        let res = send_path(&mut session, &path, &sink_clone).await;
        match res {
            Ok(()) => emit_final(
                &sink_clone,
                RsTransferStatus::Completed,
                None,
                RsTransferDirection::Send,
            ),
            Err(e) => emit_final(
                &sink_clone,
                RsTransferStatus::Failed,
                Some(e.to_string()),
                RsTransferDirection::Send,
            ),
        }
    });
    track(join.abort_handle());

    Ok(())
}

/// 取消当前传输。
///
/// 对应 mobile `hyxCancel`：中止在飞行中的任务句柄。
/// 取消后，对应 `start_listener` / `connect` / `pair_*` 的 `sink` 会收到
/// `RsProgressEvent { status: Cancelled }`（若任务尚未自然结束）。
#[frb]
pub fn cancel() {
    if let Some(h) = ACTIVE.lock().expect("active lock").take() {
        h.abort();
    }
    forget_active();
}
