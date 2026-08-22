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

use std::sync::{Arc, OnceLock};

use anyhow::Result;
use flutter_rust_bridge::frb;
use hyx_core::identity::{device_id_from_fingerprint, Identity};
use hyx_core::Uuid;

use crate::api::model::{RsDevice, RsDeviceVia};

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
    let name = format!("hyx-{}", &device_id.to_string()[..6]);
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