//! 设备发现 API。
//!
//! 对应 `mobile/src/lib.rs` 的 `hyxDiscover`：LAN UDP 信标广播 + 监听 ~2.5s，
//! 返回发现的 peer 列表。
//!
//! 与 mobile 差异：
//! - mobile 返回 `"name\tip:port\tdevice_id"` 拼接的字符串，Dart 侧再 split；
//! - FRB 版本返回 `Vec<RsDiscoveredPeer>`，Dart 侧直接拿到结构体列表。
//! - mobile 用 `runtime().block_on`；FRB 版本用 `async fn`，由 FRB 调度。

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use flutter_rust_bridge::frb;
use hyx_core::discovery::DiscoveryManager;
use hyx_core::network::udp::PeerInfo;

use crate::api::device::{current_device_id, effective_device_name, identity};
use crate::api::model::RsDiscoveredPeer;

/// 把 core 的 [`PeerInfo`] 映射为 FRB 可导出的 [`RsDiscoveredPeer`]。
fn to_rs_peer(p: PeerInfo) -> RsDiscoveredPeer {
    let addr = p.socket_addr();
    // hex 编码 32 字节指纹，供 Dart 侧 KnownDevice.fingerprint 持久化。
    // 手写实现避免给 hyx_isolates 引入 hex crate 直接依赖（core 已有，
    // 但传递依赖不能直接 use）。32 字节 → 64 个 hex 字符。
    let fingerprint: String = p
        .cert_fingerprint
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    RsDiscoveredPeer {
        name: p.device_name,
        addr: addr.to_string(),
        device_id: p.device_id,
        cert_fingerprint: p.cert_fingerprint.to_vec(),
        fingerprint,
    }
}

/// 构造并启动一个发现管理器（失败时返回 `None`，与 `discover` 的容错策略一致）。
async fn spawn_manager() -> Option<Arc<DiscoveryManager>> {
    let manager = match DiscoveryManager::new(
        effective_device_name(),
        hyx_core::DEFAULT_TRANSFER_PORT,
        identity().fingerprint(),
        current_device_id(),
        Duration::from_secs(60),
    )
    .await
    {
        Ok(m) => Arc::new(m),
        Err(e) => {
            tracing::warn!("discovery manager failed: {e}");
            return None;
        }
    };
    if let Err(e) = manager.start().await {
        tracing::warn!("discovery start failed: {e}");
        return None;
    }
    Some(manager)
}

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
    let _ = port; // port 由 core 在 DEFAULT_DISCOVERY_PORT 上用固定端口，保留参数以兼容旧调用方。
    let Some(manager) = spawn_manager().await else {
        return Ok(Vec::new());
    };
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let peers = manager.get_peers().await;
    manager.stop();

    Ok(peers.into_iter().map(to_rs_peer).collect())
}

/// 向指定 IP 单播探测，判断其对端是否在线。
///
/// 跨子网发现：不同网段间多播无法互通，调用方（Dart 蓝牙层）先通过蓝牙读到
/// 对端 IP，再调用本函数单播信标探测。对端在 ~1.5s 内回发单播信标则返回其
/// 完整身份（`Some`），否则返回 `None`（离线）。
///
/// # Arguments
/// - `ip`：对端局域网 IP（字符串，如 `"192.168.31.10"`）。
#[frb]
pub async fn probe_peer(ip: String) -> Result<Option<RsDiscoveredPeer>> {
    let target: IpAddr = match ip.parse() {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };
    let Some(manager) = spawn_manager().await else {
        return Ok(None);
    };
    let peer = manager.probe_peer(target).await.unwrap_or(None);
    manager.stop();
    Ok(peer.map(to_rs_peer))
}

/// 返回本机第一个合适的局域网 IPv4 地址（供蓝牙广播 IP 使用）。
///
/// 过滤掉 loopback 与虚拟网桥接口，与发现逻辑保持一致。无可用地址时返回 `None`。
#[frb]
pub async fn local_wifi_ip() -> Result<Option<String>> {
    Ok(hyx_core::network::udp::local_ipv4().map(|a| a.to_string()))
}
