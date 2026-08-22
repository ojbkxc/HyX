//! LAN device discovery reusing hyx-core's UDP beacon service.

use std::time::Duration;

use hyx_core::discovery::DiscoveryManager;

use crate::state;

/// A peer found on the LAN. `device_id` is the stable dedup key.
pub struct Peer {
    pub device_name: String,
    pub address: String,
    pub device_id: String,
}

/// Scan the LAN for ~2.5 s and return the peers discovered. Non-blocking to
/// Dart: runs on the shared Tokio runtime and yields via `.await`.
pub async fn discover_peers(port: Option<u16>) -> Vec<Peer> {
    let port = port.unwrap_or(hyx_core::DEFAULT_TRANSFER_PORT);
    let rt = state::runtime();
    rt.spawn(async move {
        let manager = match DiscoveryManager::new(
            state::device_name(),
            port,
            state::identity().fingerprint(),
            state::device_id(),
            Duration::from_secs(60),
        )
        .await
        {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("discovery manager failed: {e}");
                return Vec::new();
            }
        };
        if let Err(e) = manager.start().await {
            tracing::warn!("discovery start failed: {e}");
            return Vec::new();
        }
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let peers = manager.get_peers().await;
        manager.stop();
        peers
            .into_iter()
            .map(|p| {
                let address = p.socket_addr();
                Peer {
                    device_name: p.device_name,
                    address: address.to_string(),
                    device_id: p.device_id.to_string(),
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}