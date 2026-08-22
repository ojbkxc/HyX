//! 设备发现 API。
//!
//! 对应 `mobile/src/lib.rs` 的 `hyxDiscover`：LAN UDP 信标广播 + 监听 ~2.5s，
//! 返回发现的 peer 列表。
//!
//! 与 mobile 差异：
//! - mobile 返回 `"name\tip:port\tdevice_id"` 拼接的字符串，Dart 侧再 split；
//! - FRB 版本返回 `Vec<RsDiscoveredPeer>`，Dart 侧直接拿到结构体列表。
//! - mobile 用 `runtime().block_on`；FRB 版本用 `async fn`，由 FRB 调度。

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use flutter_rust_bridge::frb;
use hyx_core::discovery::DiscoveryManager;

use crate::api::device::{current_device_id, identity};
use crate::api::model::RsDiscoveredPeer;

/// 发现 LAN 上的 HyX peer。
///
/// 对应 mobile `Java_com_ojbkxc_hyx_core_HyXNative_hyxDiscover`：
/// 广播 + 监听信标 ~2.5s，返回所有应答 peer。
///
/// # Arguments
/// - `port`：发现端口（0 视为默认 14567，与 mobile 一致）。
///
/// # Errors
///
/// `DiscoveryManager` 构造或启动失败时返回空 `Vec`（与 mobile 行为一致），
/// 不向上传播错误，避免 Dart 侧因瞬态网络故障收到异常。
#[frb]
pub async fn discover(port: i32) -> Result<Vec<RsDiscoveredPeer>> {
    let port_u16 = if port > 0 {
        port as u16
    } else {
        hyx_core::DEFAULT_TRANSFER_PORT
    };

    let name = format!("hyx-{}", &current_device_id().to_string()[..6]);
    let manager = match DiscoveryManager::new(
        name,
        port_u16,
        identity().fingerprint(),
        current_device_id(),
        Duration::from_secs(60),
    )
    .await
    {
        Ok(m) => Arc::new(m),
        Err(e) => {
            tracing::warn!("discovery manager failed: {e}");
            return Ok(Vec::new());
        }
    };
    if let Err(e) = manager.start().await {
        tracing::warn!("discovery start failed: {e}");
        return Ok(Vec::new());
    }
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let peers = manager.get_peers().await;
    manager.stop();

    let result = peers
        .into_iter()
        .map(|p| RsDiscoveredPeer {
            name: p.device_name,
            addr: p.socket_addr().to_string(),
            device_id: p.device_id,
        })
        .collect();
    Ok(result)
}