//! Peer discovery module

use crate::error::Result;
use crate::identity::Fingerprint;
use crate::network::udp::{DiscoveryService, PeerInfo};
use crate::DEFAULT_DISCOVERY_PORT;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

/// 对同一来源 IP 回发单播信标的最短间隔，防止不同路由间回声引发 ping-pong 风暴。
const ECHO_THROTTLE: Duration = Duration::from_secs(2);

/// Peer discovery manager
pub struct DiscoveryManager {
    service: Arc<DiscoveryService>,
    peers: Arc<RwLock<HashMap<Uuid, PeerInfo>>>,
    peer_ttl: Duration,
    task_handles: Mutex<Option<Vec<JoinHandle<()>>>>,
    /// 上次对某来源 IP 回发单播信标的时间，用于节流。
    last_echo: Arc<Mutex<HashMap<IpAddr, Instant>>>,
}

impl DiscoveryManager {
    /// Create a new discovery manager. `cert_fingerprint` is the SHA-256
    /// of our local cert; receivers use it to pin our TLS identity when
    /// initiating a QUIC connection. `device_id` is the stable per-device
    /// identifier carried in beacons (derived from the cert fingerprint).
    pub async fn new(
        device_name: String,
        transfer_port: u16,
        cert_fingerprint: Fingerprint,
        device_id: Uuid,
        peer_ttl: Duration,
    ) -> Result<Self> {
        let service =
            DiscoveryService::new(device_name, transfer_port, cert_fingerprint, device_id).await?;

        Ok(Self {
            service: Arc::new(service),
            peers: Arc::new(RwLock::new(HashMap::new())),
            peer_ttl,
            task_handles: Mutex::new(None),
            last_echo: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Start the discovery service. Spawns the broadcaster / receiver /
    /// cleanup tasks and records their handles so [`stop`](Self::stop) can
    /// abort them and release the bound UDP socket. Returns immediately.
    /// Calling `start` again before [`stop`](Self::stop) first aborts the
    /// previous tasks so re-starting doesn't leak the old loops.
    pub async fn start(&self) -> Result<()> {
        // Abort any tasks from a prior start() so re-calling start doesn't
        // leave the old broadcaster/receiver/cleanup loops running in the
        // background (they would keep holding the UDP socket and the peers
        // write lock).
        self.stop();
        debug!("Starting discovery manager");

        // Spawn beacon broadcaster
        let broadcaster = {
            let service = Arc::clone(&self.service);
            tokio::spawn(async move {
                let mut ticker = interval(Duration::from_secs(2));
                loop {
                    ticker.tick().await;
                    if let Err(e) = service.broadcast_beacon().await {
                        warn!("Failed to broadcast beacon: {}", e);
                    }
                }
            })
        };

        // Spawn beacon receiver
        let receiver = {
            let service = Arc::clone(&self.service);
            let peers = Arc::clone(&self.peers);
            let our_device_id = service.device_id();
            let last_echo = Arc::clone(&self.last_echo);

            tokio::spawn(async move {
                loop {
                    match service.recv_beacon().await {
                        Ok((beacon, src_addr)) => {
                            // Ignore our own beacons
                            if beacon.device_id == our_device_id {
                                continue;
                            }

                            let ip = src_addr.ip();
                            // 回声应答：向来源地址单播回发本机信标（按 IP 节流），
                            // 让跨子网的单播探测端能收到回应。同一 IP 在节流窗口内
                            // 只回一次，避免多播源触发反复回声 ping-pong。跨子网时
                            // 多播到不了对端，必须用这个单播回声才能确认在线。
                            let should_echo = {
                                let mut m = last_echo.lock().expect("last_echo lock");
                                let now = Instant::now();
                                if m.get(&ip)
                                    .is_none_or(|last| now.duration_since(*last) >= ECHO_THROTTLE)
                                {
                                    m.insert(ip, now);
                                    true
                                } else {
                                    false
                                }
                            };
                            if should_echo {
                                let s = Arc::clone(&service);
                                tokio::spawn(async move {
                                    if let Err(e) = s.send_unicast_beacon(src_addr).await {
                                        warn!("Failed to echo unicast beacon to {}: {}", src_addr, e);
                                    }
                                });
                            }

                            let peer_info = PeerInfo::from((beacon.clone(), ip));

                            let mut peers_lock = peers.write().await;

                            if let Some(existing) = peers_lock.get_mut(&beacon.device_id) {
                                existing.update_last_seen();
                                trace!("Updated peer: {}", existing.device_name);
                            } else {
                                info!("Discovered new peer: {} at {}", peer_info.device_name, ip);
                                peers_lock.insert(beacon.device_id, peer_info);
                            }
                        }
                        Err(e) => {
                            warn!("Error receiving beacon: {}", e);
                        }
                    }
                }
            })
        };

        // Spawn peer cleanup task
        let cleanup = {
            let peers = Arc::clone(&self.peers);
            let ttl = self.peer_ttl;

            tokio::spawn(async move {
                let mut ticker = interval(Duration::from_secs(5));
                loop {
                    ticker.tick().await;

                    let mut peers_lock = peers.write().await;
                    let before_count = peers_lock.len();

                    peers_lock.retain(|_, peer| {
                        let alive = peer.is_alive(ttl);
                        if !alive {
                            info!("Peer timed out: {}", peer.device_name);
                        }
                        alive
                    });

                    let after_count = peers_lock.len();
                    if before_count != after_count {
                        trace!("Cleaned up {} stale peers", before_count - after_count);
                    }
                }
            })
        };

        *self.task_handles.lock().expect("task handles lock") =
            Some(vec![broadcaster, receiver, cleanup]);
        Ok(())
    }

    /// Abort all background tasks and release the bound UDP socket. Safe to
    /// call multiple times; a no-op once stopped.
    pub fn stop(&self) {
        if let Some(handles) = self.task_handles.lock().expect("task handles lock").take() {
            for h in handles {
                h.abort();
            }
        }
    }

    /// Get list of discovered peers
    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    /// Get a specific peer by ID
    pub async fn get_peer(&self, device_id: &Uuid) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(device_id).cloned()
    }

    /// Get count of discovered peers
    pub async fn peer_count(&self) -> usize {
        let peers = self.peers.read().await;
        peers.len()
    }

    /// Find peer by name
    pub async fn find_peer_by_name(&self, name: &str) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers
            .values()
            .find(|p| p.device_name.to_lowercase().contains(&name.to_lowercase()))
            .cloned()
    }

    /// 向指定 IP 的单播探测：单播本机信标到其发现端口，等待对端回声，
    /// 超时（~1.5s）内收到回应的对端即视为在线。
    ///
    /// 这是跨子网发现的探测通道：蓝牙只负责把候选 IP 交进来（见
    /// `hyx_isolates` 的 `probe_peer`），在线与否完全由这里的单播探测决定。
    /// 对端通过接收循环里的回声逻辑回发本机信标，本机据此把它加入 peers 表。
    pub async fn probe_peer(&self, target: IpAddr) -> Result<Option<PeerInfo>> {
        let addr = SocketAddr::new(target, DEFAULT_DISCOVERY_PORT);
        self.service.send_unicast_beacon(addr).await?;
        // 轮询 peers 表最多 ~1.5s，等对端回声被接收循环加入。
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let peers = self.peers.read().await;
            if let Some(p) = peers.values().find(|p| p.address == target) {
                return Ok(Some(p.clone()));
            }
        }
        Ok(None)
    }

    /// Get our device ID
    pub fn device_id(&self) -> Uuid {
        self.service.device_id()
    }

    /// Get our device name
    pub fn device_name(&self) -> &str {
        self.service.device_name()
    }
}

impl Drop for DiscoveryManager {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discovery_manager_creation() {
        let manager = DiscoveryManager::new(
            "Test Device".to_string(),
            crate::DEFAULT_TRANSFER_PORT,
            [0u8; 32],
            Uuid::new_v4(),
            Duration::from_secs(10),
        )
        .await;

        if let Ok(mgr) = manager {
            assert_eq!(mgr.device_name(), "Test Device");
            assert_eq!(mgr.peer_count().await, 0);
        }
    }

    #[tokio::test]
    async fn test_peer_operations() {
        let manager = DiscoveryManager::new(
            "Test".to_string(),
            crate::DEFAULT_TRANSFER_PORT,
            [0u8; 32],
            Uuid::new_v4(),
            Duration::from_secs(10),
        )
        .await;

        if let Ok(mgr) = manager {
            // Initially no peers
            assert_eq!(mgr.get_peers().await.len(), 0);

            // Non-existent peer
            let random_id = Uuid::new_v4();
            assert!(mgr.get_peer(&random_id).await.is_none());
        }
    }
}
