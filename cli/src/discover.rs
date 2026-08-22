//! Discovery operations.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::info;

use hyx_core::{
    discovery::DiscoveryManager,
    identity::{device_id_from_fingerprint, Identity},
};

pub async fn handle_discover(
    timeout_secs: u64,
    port: u16,
    identity_dir: Option<std::path::PathBuf>,
) -> Result<()> {
    info!("Discovering peers on network...");
    info!("  Timeout: {} seconds", timeout_secs);

    let identity = Identity::load_or_generate(identity_dir.as_deref())?;
    let device_id = device_id_from_fingerprint(&identity.fingerprint());
    let device_name = format!("cli-{}", &device_id.to_string()[..8]);
    let manager = Arc::new(
        DiscoveryManager::new(
            device_name,
            port,
            identity.fingerprint(),
            device_id,
            Duration::from_secs(10),
        )
        .await?,
    );

    manager.start().await?;

    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

    let peers = manager.get_peers().await;
    info!("Discovered {} peer(s):", peers.len());
    for (idx, peer) in peers.iter().enumerate() {
        info!(
            "  [{}] {} - {} (id={}, fp={})",
            idx + 1,
            peer.device_name,
            peer.socket_addr(),
            peer.device_id,
            hex::encode(peer.cert_fingerprint),
        );
    }

    manager.stop();
    Ok(())
}
