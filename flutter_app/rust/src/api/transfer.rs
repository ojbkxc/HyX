//! File receive/send reusing hyx-core's QUIC `P2PSession`. The heavy work runs
//! on the shared Tokio runtime while Dart stays responsive; each function
//! resolves to `Ok("")` on success or an error message.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use hyx_core::progress::ProgressState;
use hyx_core::protocol::ConfigMessage;
use hyx_core::reconnect::ReconnectConfig;
use hyx_core::session::P2PSession;
use hyx_core::transfer_folder::AcceptDecision;

use crate::state;

fn bind_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
}

/// Receive whatever a peer sends into `save_dir`.
pub async fn start_listener(save_dir: String, port: Option<u16>) -> Result<String, String> {
    let port = port.unwrap_or(hyx_core::DEFAULT_TRANSFER_PORT);
    let rt = state::runtime();
    rt.spawn(async move {
        // Broadcast beacons while listening so senders can discover us.
        let discovery = hyx_core::discovery::DiscoveryManager::new(
            state::device_name(),
            port,
            state::identity().fingerprint(),
            state::device_id(),
            std::time::Duration::from_secs(60),
        )
        .await
        .ok();

        if let Some(d) = discovery.as_ref() {
            if let Err(e) = d.start().await {
                tracing::warn!("discovery start failed: {e}");
            }
        }

        let mut session = match P2PSession::accept(bind_addr(port), state::identity(), state::device_id()).await {
            Ok(s) => s,
            Err(e) => {
                if let Some(d) = discovery.as_ref() {
                    d.stop();
                }
                return Err(e.to_string());
            }
        };

        let out = PathBuf::from(save_dir);
        let mut prog = ProgressState::new(0);
        let res = session
            .receive_to(&out, None, |_| AcceptDecision::Accept, Some(&mut prog))
            .await
            .map(|_| String::new())
            .map_err(|e| e.to_string());

        if let Some(d) = discovery.as_ref() {
            d.stop();
        }
        res
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Connect to `peer_address` (LAN discovered when empty) and send `file_path`,
/// with on-disk resume state so an interrupted send continues.
pub async fn send_file(
    peer_address: String,
    file_path: String,
    port: Option<u16>,
) -> Result<String, String> {
    let port = port.unwrap_or(hyx_core::DEFAULT_TRANSFER_PORT);
    let rt = state::runtime();
    rt.spawn(async move {
        let (addr, fp) = if peer_address.is_empty() {
            P2PSession::discover_one_peer(port, &state::identity(), state::device_id())
                .await
                .map_err(|e| e.to_string())?
        } else {
            let target = P2PSession::resolve_peer_addr(&peer_address, port)
                .await
                .map_err(|e| e.to_string())?;
            P2PSession::discover_peer(port, &state::identity(), state::device_id(), Some(target))
                .await
                .map_err(|e| e.to_string())?
        };

        let cfg = ConfigMessage {
            chunk_size: hyx_core::DEFAULT_CHUNK_SIZE,
            ..ConfigMessage::default()
        };
        let mut session = P2PSession::connect(addr, fp, state::identity(), state::device_id(), cfg)
            .await
            .map_err(|e| e.to_string())?;

        let mut prog = ProgressState::new(0);
        let src = std::path::Path::new(&file_path);
        let mut state_path = src.to_path_buf();
        if let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) {
            state_path.set_file_name(format!(".{name}.hyx-resume"));
        }
        let state_path = (src.parent().is_some()).then_some(state_path);

        session
            .send_path(src, &ReconnectConfig::default(), state_path.as_deref(), Some(&mut prog))
            .await
            .map(|_| String::new())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}