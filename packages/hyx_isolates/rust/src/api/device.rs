//! 设备身份 API。
//!
//! 对应 `mobile/src/lib.rs` 的 `hyxCreateDevice`：加载/生成 Ed25519 身份，
//! 返回证书指纹。FRB 版本返回结构化的 [`RsDevice`]，包含 `device_id`、
//! 名称、地址等，供 Dart 侧直接渲染设备卡片。
//!
//! 与 mobile 的差异：
//! - mobile 通过 `OnceLock<Arc<Identity>>` 在进程内缓存身份，返回 `jstring` 指纹；
//! - FRB 版本同样缓存，但返回 `RsDevice` 结构体，Dart 侧无需再解析字符串。
//! - 错误用 `anyhow::Result` 表达，替代 mobile 的"返回 null 表示失败"。

use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use flutter_rust_bridge::frb;
use hyx_core::Uuid;
use hyx_core::identity::{Identity, device_id_from_fingerprint};

use crate::api::model::{RsDevice, RsDeviceVia};

/// 用户自定义设备名称。`None` 时用默认 `hyx-{id前6位}`。
///
/// 与 mobile 侧 `CUSTOM_NAME` 等价：空串视为重置为默认名，
/// 非空串（trim 后）作为 beacon 中携带的 device_name。
/// 使用 `Mutex` 而非 `RwLock`：写入仅在用户改设置时发生（低频），
/// 读取发生在每次 `create_device` / `discover` / `start_listener` 时；
/// `Mutex` 在此负载下足够简单且无锁争用。
static CUSTOM_NAME: Mutex<Option<String>> = Mutex::new(None);

/// 设置自定义设备名称。空串（trim 后）视为重置为默认名 `hyx-{id前6位}`。
///
/// 对应 mobile `hyxSetDeviceName`：Dart 侧 `DeviceProvider` 在用户保存设备名时调用，
/// 后续 `createDevice` / `discover` / `startListener` 都会通过 [`effective_device_name`]
/// 拿到新名称，beacon 自然携带新名称广播给 peer。
#[frb]
pub fn set_device_name(name: String) {
    let trimmed = name.trim().to_string();
    let mut guard = CUSTOM_NAME.lock().expect("CUSTOM_NAME lock");
    *guard = if trimmed.is_empty() { None } else { Some(trimmed) };
}

/// 返回当前生效的设备名称（自定义优先，否则默认 `hyx-{id前6位}`）。
///
/// 供 `create_device` / `discovery::discover` / `transfer::start_listener` 统一调用，
/// 保证三处构造 beacon 名称的路径都用同一个名称源，避免漂移。
pub(crate) fn effective_device_name() -> String {
    let guard = CUSTOM_NAME.lock().expect("CUSTOM_NAME lock");
    if let Some(ref n) = *guard {
        return n.clone();
    }
    drop(guard);
    format!("hyx-{}", &device_id().to_string()[..6])
}

/// 进程级缓存的身份。与 mobile `IDENTITY` 等价：首次调用生成并落盘，
/// 后续调用复用，保证指纹跨重启稳定（TOFU pinning 依赖此性质）。
static IDENTITY: OnceLock<Arc<Identity>> = OnceLock::new();

pub(crate) fn identity() -> Arc<Identity> {
    IDENTITY
        .get_or_init(|| {
            Arc::new(
                Identity::load_or_generate(None)
                    .unwrap_or_else(|_| Identity::generate().expect("generate identity")),
            )
        })
        .clone()
}

/// 由证书指纹派生的稳定设备 ID。与 mobile `DEVICE_ID` 等价。
static DEVICE_ID: OnceLock<Uuid> = OnceLock::new();

pub(crate) fn device_id() -> Uuid {
    *DEVICE_ID.get_or_init(|| device_id_from_fingerprint(&identity().fingerprint()))
}

/// 创建/加载本设备身份。
///
/// 对应 mobile `Java_com_ojbkxc_hyx_core_HyXNative_hyxCreateDevice`：
/// - 首次调用：生成 Ed25519 keypair + 自签证书，落盘到 `<config_dir>/identity.{key,cert}`；
/// - 后续调用：从磁盘加载，保证指纹稳定。
///
/// 返回 [`RsDevice`]，包含 `device_id`（UUID）、`name`（`hyx-{id前6位}`，与
/// mobile `discover_peers` 命名规则一致）、`address`（空，由上层绑定后填充）、
/// `via = Lan`（默认）、`online = false`、`allow_transfer = true`。
///
/// # Errors
///
/// 仅当身份生成失败时返回错误（mobile 版本返回 null）。
#[frb]
pub fn create_device() -> Result<RsDevice> {
    let id = identity();
    let device_id = device_id();
    let name = effective_device_name();
    let _ = id.fingerprint_hex(); // 触发指纹计算，确保身份可用
    Ok(RsDevice {
        id: device_id,
        name,
        address: String::new(),
        via: RsDeviceVia::Lan,
        online: false,
        allow_transfer: true,
    })
}

/// 返回当前身份的证书指纹（hex）。
///
/// 对应 mobile `hyxCreateDevice` 的返回值（jstring 指纹）。
/// 供 Dart 侧在 QR/配对码场景展示或比对。
#[frb]
pub fn fingerprint_hex() -> String {
    identity().fingerprint_hex()
}

/// 返回当前 `device_id`。
///
/// 供 Dart 侧在不构造完整 `RsDevice` 的情况下获取稳定 ID。
#[frb]
pub fn current_device_id() -> Uuid {
    device_id()
}
