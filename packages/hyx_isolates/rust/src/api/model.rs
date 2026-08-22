//! flutter_rust_bridge 模型定义。
//!
//! 这些类型对应 `mobile/src/lib.rs` 中 JNI 边界使用的数据，但用 FRB 友好的
//! Rust 结构体/枚举表达，由 `flutter_rust_bridge_codegen` 自动生成 Dart 镜像。
//! 所有 `#[frb(mirror)]` 装饰的类型会生成对应的 `_Foo` 镜像枚举/结构体，
//! 让 Dart 侧可以按值传递而不需要 FFI 序列化原始 core 类型。

use flutter_rust_bridge::frb;
use hyx_core::Uuid;

/// 设备连接方式：局域网直连或经 rendezvous 服务器中继握手。
#[frb]
pub enum RsDeviceVia {
    /// LAN UDP 广播发现的 peer，直连 QUIC。
    Lan,
    /// 经 rendezvous 服务器交换配对码后建立连接。
    Rendezvous,
}

/// 传输方向。对应 mobile `hyxConnect`（Send）与 `hyxStartListener`/`hyxPairRendezvous`（Receive）。
#[frb]
pub enum RsTransferDirection {
    Send,
    Receive,
}

/// 传输状态机。对应 Kotlin 侧 `TransferStatus`，由 `RsProgressEvent::phase` 推进。
#[frb]
pub enum RsTransferStatus {
    /// 空闲，无活动会话。
    Idle,
    /// rendezvous 配对中（等待对端拨入或交换 code）。
    Pairing,
    /// QUIC 连接建立中（`P2PSession::connect` / `accept`）。
    Connecting,
    /// 数据传输中。
    Transferring,
    /// 成功完成。
    Completed,
    /// 失败（错误消息见 `RsProgressEvent::message`）。
    Failed,
    /// 用户取消（`hyxCancel`）。
    Cancelled,
}

/// `tracing` Level 序数，与 mobile `level_to_int` 一致：
/// 0=TRACE, 1=DEBUG, 2=INFO, 3=WARN, 4=ERROR。
#[frb]
pub enum RsLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// 一次传输的进度事件。用 `StreamSink<RsProgressEvent>` 替代 mobile 的 JNI
/// `onProgress(phase, done, total, rate)` 回调。
///
/// `phase` 沿用 mobile 语义：2 = 数据传输阶段（mobile 现仅发送 phase=2）。
/// `transferred` / `total` 为字节，`speed` 为字节/秒（mobile 中为 i64，此处用 f64
/// 以避免 FRB 对负值的额外处理）。
#[frb]
pub struct RsProgressEvent {
    /// 传输阶段，对应 mobile `onProgress` 的第一个 int 参数。
    pub phase: i32,
    /// 已传输字节数。
    pub transferred: u64,
    /// 总字节数（未知时为 0）。
    pub total: u64,
    /// 瞬时速率（字节/秒）。
    pub speed: f64,
    /// 当前状态（用于状态机推进；mobile 版本通过 Done 事件隐式表达）。
    pub status: RsTransferStatus,

    /// 失败/完成时的可选消息（对应 mobile `Evt::Done` 的 `Result<String, String>`）。
    pub message: Option<String>,
}

/// 日志事件。用 `StreamSink<RsLogEvent>` 替代 mobile 的 JNI `onLog(level, tag, msg)` 回调。
#[frb]
pub struct RsLogEvent {
    pub level: RsLogLevel,
    pub tag: String,
    pub message: String,
}

/// 本设备信息。对应 mobile `hyxCreateDevice` 返回的指纹 + 派生 `device_id`。
#[frb]
pub struct RsDevice {
    /// 由证书指纹派生的稳定 UUID（mobile `device_id()`）。
    pub id: Uuid,
    /// 设备显示名（mobile 中为 `hyx-{device_id前6位}`）。
    pub name: String,
    /// 监听地址（`0.0.0.0:port` 或具体地址）。
    pub address: String,
    /// 连接方式。
    pub via: RsDeviceVia,
    /// 是否在线（监听中）。
    pub online: bool,
    /// 是否允许接收对端文件（mobile 中恒为 Accept）。
    pub allow_transfer: bool,
}

/// 发现到的 peer。对应 mobile `hyxDiscover` 返回的 `"name\tip:port\tdevice_id"` 行。
#[frb]
pub struct RsDiscoveredPeer {
    /// peer 自报名称。
    pub name: String,
    /// peer 的 `ip:port` 字符串。
    pub addr: String,
    /// peer 的稳定设备 ID。
    pub device_id: Uuid,
}
