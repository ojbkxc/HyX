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
//!
//! # 为什么每个网络接口一个 socket
//!
//! 一个 UDP 多播 socket 只能在一个接口上 `join_multicast_v4` 并 `set_multicast_if_v4`。
//! 当手机开热点时，手机同时有 WiFi 接口和热点 AP 接口，二者属于不同子网。如果只在
//! 一个接口上 join 多播组，热点子网上的设备收不到多播包，手机也收不到热点子网上
//! 设备发的多播包 → 互相都搜索不到。
//!
//! 解决办法（参考 LocalSend）：遍历所有合适的网络接口，**每个接口创建一个独立的
//! socket**，全部 wildcard 绑定 `0.0.0.0:port`（配合 `SO_REUSEADDR`/`SO_REUSEPORT`），
//! 各自 join 多播组并 pin 到自己的接口。发送时在所有 socket 上都发，接收时任意一个
//! socket 收到即可。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
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
    /// 每个网络接口一个多播 socket，全部 wildcard 绑定同一发现端口。
    /// 发送时在所有 socket 上都发；接收时任意一个收到即可。
    sockets: Vec<Arc<UdpSocket>>,
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
        // 所有 socket 都 wildcard 绑定 0.0.0.0:port，配合 SO_REUSEADDR/SO_REUSEPORT
        // 让多个 socket 能同时绑定同一端口。
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), discovery_port);
        let multicast_addr = SocketAddr::new(IpAddr::V4(MULTICAST_GROUP), discovery_port);

        trace!("Creating discovery service on port {}", discovery_port);

        // 枚举所有合适的 IPv4 接口，每个接口创建一个独立的多播 socket。
        let iface_ips = list_multicast_interfaces_ipv4();
        let mut sockets: Vec<Arc<UdpSocket>> = Vec::new();

        for iface_ip in iface_ips {
            trace!(
                "为接口 {} 创建多播 socket (group {}, port {})",
                iface_ip, MULTICAST_GROUP, discovery_port
            );

            // 每一步失败只 warn 不致命，跳过该接口继续下一个 —— 单个不可用接口
            // （如虚拟适配器）不应让整个发现服务挂掉。
            let sock = match Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        "在接口 {} 上创建 socket 失败: {}, 跳过该接口",
                        iface_ip, e
                    );
                    continue;
                }
            };

            // SO_REUSEADDR：允许多个 socket 同时绑定同一发现端口。
            if let Err(e) = sock.set_reuse_address(true) {
                warn!("set_reuse_address(true) on {} 失败: {}", iface_ip, e);
            }

            // Unix 上设置 SO_REUSEPORT，让多个 socket 能真正同时绑定同一端口
            // （Windows 没有 SO_REUSEPORT，只有 SO_REUSEADDR）。
            #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
            if let Err(e) = sock.set_reuse_port(true) {
                warn!("set_reuse_port(true) on {} 失败: {}", iface_ip, e);
            }

            // wildcard 绑定 0.0.0.0:port —— 这是让 socket 能收到发往多播组的包的关键，
            // 平台会把目的地址和绑定地址做匹配，绑到接口地址反而收不到多播包。
            if let Err(e) = sock.bind(&SockAddr::from(bind_addr)) {
                warn!(
                    "bind {} on interface {} 失败: {}, 跳过该接口",
                    bind_addr, iface_ip, e
                );
                continue;
            }

            // join 让本 socket 收到发往该组的多播包；失败则本机收不到该接口上的别人 beacon，
            // 这个 socket 没意义了，跳过。
            if let Err(e) = sock.join_multicast_v4(&MULTICAST_GROUP, &iface_ip) {
                warn!(
                    "join_multicast_v4({}) on {} 失败: {}, 跳过该接口",
                    MULTICAST_GROUP, iface_ip, e
                );
                continue;
            }

            // 指定从该网卡发出多播包，对发送方关键（多接口机器否则可能走默认路由，
            // 所有 socket 都从同一接口出，热点子网上的设备还是收不到）。
            if let Err(e) = sock.set_multicast_if_v4(&iface_ip) {
                warn!(
                    "set_multicast_if_v4({}) 失败: {}, 多播可能从错误接口发出",
                    iface_ip, e
                );
            }

            // 开启回环：同机多实例（如集成测试）能收到自己发的多播包。
            if let Err(e) = sock.set_multicast_loop_v4(true) {
                warn!("set_multicast_loop_v4(true) on {} 失败: {}", iface_ip, e);
            }

            // TTL=1 限制在本地子网，不跨路由器泄漏。
            if let Err(e) = sock.set_multicast_ttl_v4(1) {
                warn!("set_multicast_ttl_v4(1) on {} 失败: {}", iface_ip, e);
            }

            if let Err(e) = sock.set_nonblocking(true) {
                warn!("set_nonblocking(true) on {} 失败: {}", iface_ip, e);
            }

            let std_socket: std::net::UdpSocket = sock.into();
            let tokio_socket = match UdpSocket::from_std(std_socket) {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        "UdpSocket::from_std on {} 失败: {}, 跳过该接口",
                        iface_ip, e
                    );
                    continue;
                }
            };
            sockets.push(Arc::new(tokio_socket));
        }

        if sockets.is_empty() {
            return Err(Error::Protocol(
                "未能为任何网络接口创建多播 socket，多播发现无法工作".into(),
            ));
        }

        trace!("共为 {} 个网络接口创建多播 socket", sockets.len());

        Ok(Self {
            sockets,
            device_id,
            device_name,
            transfer_port,
            cert_fingerprint,
            multicast_addr,
        })
    }

    fn create_beacon(&self) -> DiscoveryBeacon {
        // 优先用全局自定义名称（运行时可通过 `core::set_device_name` 更新），
        // 这样常驻的 DiscoveryManager（如 hyxStartListener 创建的）在用户改了
        // 自定义名称后也能即时广播新名称，无需重启。fallback 到构造时的名称。
        let device_name = crate::try_custom_device_name().unwrap_or_else(|| self.device_name.clone());
        DiscoveryBeacon {
            version: PROTOCOL_VERSION,
            device_id: self.device_id,
            device_name,
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

        // 在所有 socket 上都发一遍 —— 每个 socket pin 到不同接口，这样多播包会从
        // 每个接口的子网发出，覆盖所有相连的链路（WiFi 子网 + 热点子网等）。
        for socket in &self.sockets {
            trace!("Sending multicast beacon to {}", self.multicast_addr);
            if let Err(e) = socket.send_to(&data, self.multicast_addr).await {
                warn!("Failed to send multicast beacon on one socket: {}", e);
            }
        }
        Ok(())
    }

    pub async fn recv_beacon(&self) -> Result<(DiscoveryBeacon, SocketAddr)> {
        // 并行在所有 socket 上 recv_from，任意一个先收到就返回。
        // JoinSet 在取到第一个成功结果后 drop，会 abort 其余 task —— 这没问题，
        // DiscoveryManager 的 receiver loop 每次都重新调用 recv_beacon，下次重新 spawn。
        // Arc<UdpSocket> 的 clone 会在 task abort 时被 drop。
        let mut join_set = tokio::task::JoinSet::new();
        for socket in &self.sockets {
            let socket = Arc::clone(socket);
            join_set.spawn(async move {
                let mut buf = vec![0u8; MAX_PACKET_SIZE];
                socket
                    .recv_from(&mut buf)
                    .await
                    .map(|(len, addr)| (buf, len, addr))
            });
        }

        let (buf, len, src_addr) = loop {
            match join_set.join_next().await {
                Some(Ok(Ok((buf, len, addr)))) => break (buf, len, addr),
                Some(Ok(Err(e))) => {
                    warn!("recv_from error on one socket: {}", e);
                    continue;
                }
                Some(Err(e)) => {
                    warn!("join error: {}", e);
                    continue;
                }
                None => return Err(Error::Protocol("All discovery sockets failed".into())),
            }
        };

        let beacon: DiscoveryBeacon = rmp_serde::from_slice(&buf[..len])
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

    /// 向指定地址单播发送本机信标。
    ///
    /// 用于跨子网探测：不同网段间多播无法互通，但单播 UDP 可以在路由可达的
    /// 局域网内到达对端。接收方收到后会对来源地址回发单播信标（回声），
    /// 探测端据此确认对端在线并拿到完整身份（device_id/指纹/名称）。
    pub async fn send_unicast_beacon(&self, target: SocketAddr) -> Result<()> {
        let beacon = self.create_beacon();
        let data = rmp_serde::to_vec(&beacon)?;
        if data.len() > MAX_PACKET_SIZE {
            return Err(Error::Protocol(format!(
                "Beacon too large: {} bytes",
                data.len()
            )));
        }
        for socket in &self.sockets {
            if let Err(e) = socket.send_to(&data, target).await {
                warn!("Failed to send unicast beacon to {}: {}", target, e);
            }
        }
        Ok(())
    }
}

/// 返回第一个合适的非虚拟 IPv4 局域网接口地址。
///
/// 与 [`list_multicast_interfaces_ipv4`] 使用相同的过滤规则，供蓝牙分享本机 IP、
/// 或者其它需要对外广播本机可达地址的场景使用。
pub fn local_ipv4() -> Option<IpAddr> {
    let ifaces = if_addrs::get_if_addrs().ok()?;
    for iface in &ifaces {
        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
            if v4.ip.is_loopback() {
                continue;
            }
            if looks_virtual(&iface.name) {
                continue;
            }
            return Some(IpAddr::V4(v4.ip));
        }
    }
    None
}

/// 判断接口名是否明显是虚拟网桥/虚拟适配器。
fn looks_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("docker")
        || n.starts_with("veth")
        || n.starts_with("vbox")
        || n.starts_with("vmnet")
        || n.starts_with("virbr")
        || n.starts_with("br-")
        || n.starts_with("tap")
        || n == "lo"
}

/// 列出所有适合加入多播组的 IPv4 接口地址。
///
/// 过滤规则：
/// 1. 跳过 loopback（`127.0.0.0/8`）
/// 2. 跳过名字明显是虚拟网桥的接口（`docker*`、`veth*`、`vbox*`、`vmnet*`、
///    `virbr*`、`br-*`、`tap*`、`lo`）—— 这些通常不是真实链路，加入多播组
///    反而可能干扰真实接口的发现
/// 3. 其余每个非 loopback IPv4 接口都返回，调用方会为每个接口创建独立 socket
///
/// 返回空 Vec 表示没找到任何合适接口，调用方应返回 Error。
fn list_multicast_interfaces_ipv4() -> Vec<Ipv4Addr> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(v) => v,
        Err(e) => {
            warn!("get_if_addrs failed: {}, 多播发现可能不工作", e);
            return Vec::new();
        }
    };

    let mut result = Vec::new();
    for iface in &ifaces {
        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
            if v4.ip.is_loopback() {
                continue;
            }
            if looks_virtual(&iface.name) {
                trace!("跳过虚拟网桥接口 {} ({})", iface.name, v4.ip);
                continue;
            }
            trace!("候选多播接口 {} ({})", iface.name, v4.ip);
            result.push(v4.ip);
        }
    }

    if result.is_empty() {
        warn!(
            "未找到任何合适的 IPv4 接口加入多播组 {}，多播发现可能不工作",
            MULTICAST_GROUP
        );
    }
    result
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
