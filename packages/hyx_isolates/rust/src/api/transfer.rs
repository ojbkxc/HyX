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

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use flutter_rust_bridge::frb;
use hyx_core::progress::{ProgressCallback, ProgressState};
use hyx_core::protocol::ConfigMessage;
use hyx_core::reconnect::ReconnectConfig;
use hyx_core::session::P2PSession;
use hyx_core::transfer_folder::AcceptDecision;
use hyx_core::DEFAULT_RENDEZVOUS_PORT;
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

/// 被禁止接收的设备 ID 列表（由 Dart 侧 `DeviceProvider` 同步）。
///
/// - `None`：尚未设置过，视为"无过滤"，保持向后兼容（所有设备都被允许）。
/// - `Some(set)`：集合中的设备 ID（Uuid 字符串）会被 `receive_into` 拒收。
///
/// 使用全局 `Mutex` 而非 `RwLock`：写入仅在用户切换 UI 开关时发生（低频），
/// 读取发生在每次接收开始时；`Mutex` 在此负载下足够简单且无锁争用。
static BLOCKED_DEVICES: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// 更新被禁止接收的设备 ID 列表。对应 Dart 侧 `DeviceProvider.allowReceive == false` 的设备。
///
/// 由 Dart 侧在 `StartAutoListenAction` / `StartReceiveAction` 触发，传入当前完整的
/// 禁止列表（全量替换而非增量）。fire-and-forget：调用方不等待返回。
///
/// # Arguments
/// - `ids`：被禁止的设备 ID（Uuid 字符串）列表。空列表表示"无禁止"。
#[frb]
pub fn set_blocked_devices(ids: Vec<String>) {
    *BLOCKED_DEVICES.lock().expect("blocked lock") = Some(ids.into_iter().collect());
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

/// 把 32 字节指纹 hex 编码为 String（64 个小写 hex 字符）。
///
/// 手写实现避免给 hyx_isolates 引入 `hex` crate 直接依赖（`hyx_core` 已有，
/// 但传递依赖不能直接 `use`）。与 `discovery::discover` 中填充
/// `RsDiscoveredPeer.fingerprint` 的编码方式一致。
fn encode_fingerprint_hex(fp: &hyx_core::identity::Fingerprint) -> String {
    fp.iter().map(|b| format!("{b:02x}")).collect()
}

/// 把 hex 字符串解码为 32 字节指纹。
///
/// 供 `connect` 的 `cached_fingerprint` 参数解析：Dart 侧 `KnownDevice.fingerprint`
/// 持久化的 hex 字符串 → `P2PSession::connect` 需要的 `[u8; 32]`。
///
/// # Errors
///
/// 返回 `Err(())` 当：
/// - 字符串长度 ≠ 64（32 字节 × 2 hex 字符）；或
/// - 包含非 hex 字符。
///
/// 调用方应把 `Err` 视为"缓存指纹无效，回退到发现/TOFU 路径"，不向用户报错。
fn decode_fingerprint_hex(hex: &str) -> std::result::Result<hyx_core::identity::Fingerprint, ()> {
    if hex.len() != 64 {
        return Err(());
    }
    let mut fp = [0u8; 32];
    let bytes = hex.as_bytes();
    for i in 0..32 {
        let hi = match (bytes[i * 2] as char).to_digit(16) {
            Some(v) => v as u8,
            None => return Err(()),
        };
        let lo = match (bytes[i * 2 + 1] as char).to_digit(16) {
            Some(v) => v as u8,
            None => return Err(()),
        };
        fp[i] = (hi << 4) | lo;
    }
    Ok(fp)
}

/// 在 TOFU 连接成功后，通过 `sink` 把对端实际指纹回传给 Dart 侧缓存。
///
/// 复用 `RsProgressEvent` 携带 `peer_fingerprint: Some(hex)`，`status: Connecting`
/// 表示"连接已建立，正在回填缓存"。Dart 侧 `transfer_provider` 监听到非 `None`
/// 时把它写入对应 `KnownDevice.fingerprint` 持久化，后续连接直接 pin 跳过发现。
///
/// 此事件在 `send_path` 之前发出，不携带字节进度（`transferred`/`total` 为 0）。
fn emit_peer_fingerprint_cached(sink: &StreamSink<RsProgressEvent>, fp_hex: String) {
    let _ = sink.add(RsProgressEvent {
        direction: RsTransferDirection::Send,
        phase: 1,
        transferred: 0,
        total: 0,
        speed: 0.0,
        status: RsTransferStatus::Connecting,
        message: None,
        peer_fingerprint: Some(fp_hex),
    });
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
            peer_fingerprint: None,
        });
    })
}

/// 接收对端发来的文件到 `dir`。对应 mobile `receive_into`。
///
/// 在调用 `receive_to` 前先检查发送方设备 ID 是否在 [`BLOCKED_DEVICES`] 中：
/// 若被禁止则向 `sink` 推送 `Failed` 事件并返回 `Err`，不进入实际传输流程。
/// 这样被禁止的设备连一个 chunk 都不会写入磁盘。
async fn receive_into(
    session: &mut P2PSession,
    dir: &str,
    sink: &StreamSink<RsProgressEvent>,
) -> Result<()> {
    // 取发送方设备 ID（Uuid 字符串），与 Dart 侧 KnownDevice.deviceId 比对。
    let peer_id = session.peer_device_id().to_string();
    {
        let blocked = BLOCKED_DEVICES.lock().expect("blocked lock");
        if let Some(set) = blocked.as_ref() {
            if set.contains(&peer_id) {
                let msg = format!("设备 {peer_id} 已被禁止接收");
                emit_final(
                    sink,
                    RsTransferStatus::Failed,
                    Some(msg),
                    RsTransferDirection::Receive,
                );
                return Err(anyhow::anyhow!("blocked device: {peer_id}"));
            }
        }
    }

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
        peer_fingerprint: None,
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
                tracing::warn!("accept failed on port {port_u16}: {e}");
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
/// # 决策树（与 design.md §1.2 一致）
///
/// - `peer_address` 非空 + `cached_fingerprint` 非空：直接 pin 连接，跳过 UDP 发现。
///   pin 失败（`FingerprintMismatch`，peer 换了 identity）→ 回退 TOFU 重新信任。
/// - `peer_address` 非空 + `cached_fingerprint` 空：短超时发现拿指纹 → pin 连接；
///   发现失败 → TOFU 回退直连。
/// - `peer_address` 空：自动发现任意 peer → pin 连接（原行为）。
///
/// TOFU 连接成功后，通过 `RsProgressEvent.peer_fingerprint` 把对端实际指纹回传给
/// Dart 侧缓存，后续连接直接 pin 跳过发现。
///
/// # Arguments
/// - `peer_address`：对端地址（`ip:port` 或空串触发自动发现）。
/// - `file_path`：待发送文件路径。
/// - `chunk_bytes` / `compression`：传输参数。
/// - `port`：对端端口（0 视为默认 14567）。
/// - `cached_fingerprint`：可选缓存 fingerprint（hex）。非空且 `peer_address` 非空时
///   直接 pin 连接，跳过 UDP 发现。空/`None` 视为无缓存。
/// - `sink`：进度事件流。
#[frb]
pub fn connect(
    peer_address: String,
    file_path: String,
    chunk_bytes: i32,
    compression: i32,
    port: i32,
    cached_fingerprint: Option<String>,
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
        // ---- 阶段 1：解析地址 + 决定 fingerprint 来源 ----
        // fp_option: Some(fp) → 走 P2PSession::connect (pin)
        //             None    → 走 P2PSession::connect_tofu (TOFU 回退)
        let (addr, fp_option) = if !peer.is_empty() {
            // 有明确地址 → resolve
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

            // 缓存指纹非空且非空串 → 解析后直接用
            if let Some(fp_hex) = cached_fingerprint.as_ref().filter(|s| !s.is_empty()) {
                match decode_fingerprint_hex(fp_hex) {
                    Ok(fp) => (target, Some(fp)),
                    Err(_) => {
                        // 缓存指纹无效 → 回退到短超时发现拿指纹
                        match P2PSession::discover_peer(
                            port_u16,
                            &identity(),
                            current_device_id(),
                            Some(target),
                        )
                        .await
                        {
                            Ok((a, f)) => (a, Some(f)),
                            Err(_) => {
                                // 发现也失败 → TOFU 回退（无指纹直连）
                                (target, None)
                            }
                        }
                    }
                }
            } else {
                // 无缓存指纹 → 短超时发现拿指纹
                match P2PSession::discover_peer(
                    port_u16,
                    &identity(),
                    current_device_id(),
                    Some(target),
                )
                .await
                {
                    Ok((a, f)) => (a, Some(f)),
                    Err(_) => {
                        // 发现失败 → TOFU 回退
                        (target, None)
                    }
                }
            }
        } else {
            // 无明确地址 → 自动发现任意 peer（原行为）
            match P2PSession::discover_one_peer(port_u16, &identity(), current_device_id()).await {
                Ok((a, f)) => (a, Some(f)),
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

        // ---- 阶段 2：建立 session ----
        // pin 路径失败 FingerprintMismatch → 回退 TOFU 重新信任
        // 注意：cfg 被 P2PSession::connect 消耗，回退路径需 cfg.clone()
        let mut session = match fp_option {
            Some(fp) => {
                // 预留一份给 FingerprintMismatch 回退的 connect_tofu
                let cfg_for_tofu = cfg.clone();
                match P2PSession::connect(addr, fp, identity(), current_device_id(), cfg).await {
                    Ok(s) => s,
                    Err(hyx_core::Error::FingerprintMismatch) => {
                        // peer 换了 identity → 回退 TOFU 重新信任新指纹
                        match P2PSession::connect_tofu(
                            addr,
                            identity(),
                            current_device_id(),
                            cfg_for_tofu,
                        )
                        .await
                        {
                            Ok(s) => {
                                // 回传新指纹给 Dart 缓存，覆盖旧指纹
                                emit_peer_fingerprint_cached(
                                    &sink_clone,
                                    encode_fingerprint_hex(&s.peer_fingerprint()),
                                );
                                s
                            }
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
                    }
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
            }
            None => match P2PSession::connect_tofu(addr, identity(), current_device_id(), cfg).await
            {
                Ok(s) => {
                    // TOFU 首次信任 → 回传实际指纹给 Dart 缓存
                    emit_peer_fingerprint_cached(
                        &sink_clone,
                        encode_fingerprint_hex(&s.peer_fingerprint()),
                    );
                    s
                }
                Err(e) => {
                    emit_final(
                        &sink_clone,
                        RsTransferStatus::Failed,
                        Some(e.to_string()),
                        RsTransferDirection::Send,
                    );
                    return;
                }
            },
        };

        // ---- 阶段 3：发送文件 ----
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

/// 直连发送文件，使用已知的对端指纹，跳过 discovery。
///
/// 从 UI 设备列表发送时调用，避免与 `start_listener` 的 DiscoveryManager 端口冲突。
///
/// # Arguments
/// - `peer_address`：对端地址（`ip:port`）。
/// - `peer_fingerprint`：对端证书指纹（32 字节，来自 `RsDiscoveredPeer.cert_fingerprint`）。
/// - `file_path`：待发送文件路径。
/// - `chunk_bytes` / `compression`：传输参数。
/// - `port`：对端端口（0 视为默认 14567）。
/// - `sink`：进度事件流。
#[frb]
pub fn connect_direct(
    peer_address: String,
    peer_fingerprint: Vec<u8>,
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
    let path = file_path.clone();
    let sink_clone = sink.clone();

    let join = runtime().spawn(async move {
        let peer_addr = match P2PSession::resolve_peer_addr(&peer_address, port_u16).await {
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
        let fp: [u8; 32] = match peer_fingerprint.as_slice().try_into() {
            Ok(arr) => arr,
            Err(_) => {
                emit_final(
                    &sink_clone,
                    RsTransferStatus::Failed,
                    Some("invalid fingerprint length".to_string()),
                    RsTransferDirection::Send,
                );
                return;
            }
        };
        let mut session =
            match P2PSession::connect(peer_addr, fp, identity(), current_device_id(), cfg).await {
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
            peer_fingerprint: None,
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
            peer_fingerprint: None,
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
