//! UDP multicast for LAN peer discovery.
//!
//! Beacons now carry the sender's certificate fingerprint so the receiver
//! has everything it needs to pin the peer's TLS cert when initiating a
//! QUIC connection.
//!
//! # 为什么用多播而不是广播
//!
//! 之前的实现用 `255.255.255.255` 广播做 LAN 发现。问题：当电脑连手机热点时，
//! 手机作为 AP 热点主机广播 beacon，但许多 Android AP 实现的"客户端隔离"会
//! 把广播包丢弃，不转发给连接的客户端 → 电脑收不到手机的 beacon。
//!
//! UDP 多播（`224.0.0.167`）绕过这个限制：多播是 IGMP 加入的组，AP 会把
//! 多播包按组转发给所有加入了该组的客户端。参考 localsend 的实现。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::net::UdpSocket;
use tracing::{trace, warn};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::identity::Fingerprint;
use crate::protocol::DiscoveryBeacon;
use crate::{DEFAULT_DISCOVERY_PORT, PROTOCOL_VERSION};

const MAX_PACKET_SIZE: usize = 1500;

/// 多播组地址 — 在 `224.0.0.0/24` 范围内。
///
/// 选 `224.0.0.167` 是因为 localsend 用这个地址，且某些 Android 设备的
/// WiFi 多播过滤只放行 `224.0.0.0/24` 这个链路本地范围的多播包。
/// 端口保持 `DEFAULT_DISCOVERY_PORT`（14566）。
const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);

pub struct DiscoveryService {
    socket: UdpSocket,
    device_id: Uuid,
    device_name: String,
    transfer_port: u16,
    cert_fingerprint: Fingerprint,
    multicast_addr: SocketAddr,
}

impl DiscoveryService {
    pub async fn new(
        device_name: String,
        transfer_port: u16,
        cert_fingerprint: Fingerprint,
        device_id: Uuid,
    ) -> Result<Self> {
        let discovery_port = DEFAULT_DISCOVERY_PORT;
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), discovery_port);

        trace!("Creating discovery service on port {}", discovery_port);
        // 用 socket2 创建 socket 并设置 SO_REUSEADDR，允许多个 socket 同时绑定
        // 同一发现端口（如监听中 + 发送方临时 discovery 同时存在），避免端口冲突。
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        sock.set_reuse_address(true)?;
        sock.set_nonblocking(true)?;
        sock.bind(&SockAddr::from(bind_addr))?;

        // 多播配置：选一个合适的 IPv4 接口，加入多播组并设置发送接口。
        // 顺序参考 localsend：bind → join → set_multicast_if → set_multicast_loop → set_multicast_ttl。
        // 每一步失败都只 warn 不致命 —— 发送多播不需要 join，socket 仍可绑定收发普通包。
        let multicast_addr = SocketAddr::new(IpAddr::V4(MULTICAST_GROUP), discovery_port);
        match pick_multicast_interface_ipv4() {
            Some(iface_ip) => {
                trace!(
                    "Joining multicast group {} on interface {}",
                    MULTICAST_GROUP,
                    iface_ip
                );
                // join 让本 socket 收到发往该组的多播包；失败则本机收不到别人的 beacon。
                if let Err(e) = sock.join_multicast_v4(&MULTICAST_GROUP, &iface_ip) {
                    warn!(
                        "join_multicast_v4({}) on {} failed: {}, 本机可能收不到多播 beacon",
                        MULTICAST_GROUP, iface_ip, e
                    );
                }
                // 指定从哪个网卡发出多播包，对发送方关键（多接口机器否则可能走默认路由）。
                if let Err(e) = sock.set_multicast_if_v4(&iface_ip) {
                    warn!(
                        "set_multicast_if_v4({}) failed: {}, 多播可能从错误接口发出",
                        iface_ip, e
                    );
                }
                // 开启回环：同机多实例（如集成测试）能收到自己发的多播包。
                if let Err(e) = sock.set_multicast_loop_v4(true) {
                    warn!("set_multicast_loop_v4(true) failed: {}", e);
                }
                // TTL=1 限制在本地子网，不跨路由器泄漏。
                if let Err(e) = sock.set_multicast_ttl_v4(1) {
                    warn!("set_multicast_ttl_v4(1) failed: {}", e);
                }
            }
            None => {
                warn!(
                    "未找到合适的 IPv4 接口加入多播组 {}，多播发现可能不工作",
                    MULTICAST_GROUP
                );
            }
        }

        let std_socket: std::net::UdpSocket = sock.into();
        let socket = tokio::net::UdpSocket::from_std(std_socket)?;

        Ok(Self {
            socket,
            device_id,
            device_name,
            transfer_port,
            cert_fingerprint,
            multicast_addr,
        })
    }

    fn create_beacon(&self) -> DiscoveryBeacon {
        DiscoveryBeacon {
            version: PROTOCOL_VERSION,
            device_id: self.device_id,
            device_name: self.device_name.clone(),
            port: self.transfer_port,
            cert_fingerprint: self.cert_fingerprint,
        }
    }

    pub async fn broadcast_beacon(&self) -> Result<()> {
        let beacon = self.create_beacon();
        let data = rmp_serde::to_vec(&beacon)?;

        if data.len() > MAX_PACKET_SIZE {
            return Err(Error::Protocol(format!(
                "Beacon too large: {} bytes",
                data.len()
            )));
        }

        trace!("Sending multicast beacon to {}", self.multicast_addr);
        self.socket.send_to(&data, self.multicast_addr).await?;
        Ok(())
    }

    pub async fn recv_beacon(&self) -> Result<(DiscoveryBeacon, SocketAddr)> {
        let mut buf = vec![0u8; MAX_PACKET_SIZE];
        let (len, src_addr) = self.socket.recv_from(&mut buf).await?;
        buf.truncate(len);

        let beacon: DiscoveryBeacon = rmp_serde::from_slice(&buf)
            .map_err(|e| Error::Protocol(format!("Invalid beacon: {}", e)))?;

        if beacon.version != PROTOCOL_VERSION {
            warn!(
                "Received beacon with incompatible version {} from {}",
                beacon.version, src_addr
            );
            return Err(Error::VersionMismatch {
                peer: beacon.version,
                ours: PROTOCOL_VERSION,
            });
        }

        trace!("Received beacon from {} ({})", beacon.device_name, src_addr);
        Ok((beacon, src_addr))
    }

    pub fn device_id(&self) -> Uuid {
        self.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }
}

/// 从系统网络接口中挑选一个适合做多播的 IPv4 接口地址。
///
/// 选择策略（按优先级）：
/// 1. 跳过 loopback（`127.0.0.0/8`）和名字明显是虚拟网桥的接口
///    （`docker*`、`veth*`、`vbox*`、`vmnet*`、`virbr*`、`br-*`、`tap*`）
/// 2. 回退到第一个非 loopback 的 IPv4 地址（即使是虚拟接口也认）
/// 3. 实在找不到返回 `None`，调用方应回退到只绑定 socket 不 join 多播
fn pick_multicast_interface_ipv4() -> Option<Ipv4Addr> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(v) => v,
        Err(e) => {
            warn!("get_if_addrs failed: {}, 多播发现可能不工作", e);
            return None;
        }
    };

    let looks_virtual = |name: &str| {
        let n = name.to_ascii_lowercase();
        n.starts_with("docker")
            || n.starts_with("veth")
            || n.starts_with("vbox")
            || n.starts_with("vmnet")
            || n.starts_with("virbr")
            || n.starts_with("br-")
            || n.starts_with("tap")
            || n == "lo"
    };

    let mut first_non_loopback: Option<Ipv4Addr> = None;
    for iface in &ifaces {
        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
            if v4.ip.is_loopback() {
                continue;
            }
            if first_non_loopback.is_none() {
                first_non_loopback = Some(v4.ip);
            }
            if !looks_virtual(&iface.name) {
                trace!(
                    "Selected multicast interface {} ({})",
                    iface.name,
                    v4.ip
                );
                return Some(v4.ip);
            }
        }
    }

    if first_non_loopback.is_none() {
        warn!("未找到任何非 loopback 的 IPv4 接口，多播发现可能不工作");
    }
    first_non_loopback
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub device_id: Uuid,
    pub device_name: String,
    pub address: IpAddr,
    pub port: u16,
    pub cert_fingerprint: Fingerprint,
    pub last_seen: SystemTime,
}

impl PeerInfo {
    pub fn is_alive(&self, ttl: Duration) -> bool {
        match SystemTime::now().duration_since(self.last_seen) {
            Ok(elapsed) => elapsed < ttl,
            Err(_) => false,
        }
    }

    pub fn update_last_seen(&mut self) {
        self.last_seen = SystemTime::now();
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.address, self.port)
    }
}

impl From<(DiscoveryBeacon, IpAddr)> for PeerInfo {
    fn from((beacon, address): (DiscoveryBeacon, IpAddr)) -> Self {
        Self {
            device_id: beacon.device_id,
            device_name: beacon.device_name,
            address,
            port: beacon.port,
            cert_fingerprint: beacon.cert_fingerprint,
            last_seen: SystemTime::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_beacon() -> DiscoveryBeacon {
        DiscoveryBeacon {
            version: PROTOCOL_VERSION,
            device_id: Uuid::new_v4(),
            device_name: "Test".to_string(),
            port: crate::DEFAULT_TRANSFER_PORT,
            cert_fingerprint: [0u8; 32],
        }
    }

    #[test]
    fn peer_info_lifetime() {
        let mut peer = PeerInfo::from((sample_beacon(), IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(peer.is_alive(Duration::from_secs(60)));
        peer.update_last_seen();
        assert!(peer.is_alive(Duration::from_secs(60)));
    }

    #[test]
    fn peer_socket_addr_matches_beacon_port() {
        let peer = PeerInfo::from((sample_beacon(), IpAddr::V4(Ipv4Addr::LOCALHOST)));
        let addr = peer.socket_addr();
        assert_eq!(addr.port(), crate::DEFAULT_TRANSFER_PORT);
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
}
