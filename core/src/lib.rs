//! P2P Core Library
//!
//! This crate provides the core functionality for peer-to-peer file transfers
//! with compression and resume capabilities.

pub mod bandwidth;
pub mod compression;
pub mod discovery;
pub mod error;
pub mod handshake;
pub mod history;
pub mod identity; // Ed25519 device identity + self-signed cert
pub mod known_peers; // TOFU fingerprint trust store
pub mod network;
pub mod progress;
pub mod protocol;
pub mod reconnect;
pub mod session;
pub mod state;
pub mod tls; // rustls config + fingerprint-pinning verifier
pub mod transfer_file;
pub mod transfer_folder;
pub mod traversal; // STUN + hole punch + rendezvous orchestration
pub mod verification;

pub use error::{Error, Result};
pub use protocol::Message;

// Re-export commonly used types
pub use uuid::Uuid;

/// Protocol version. Bumped to 3 to drop the now-unused `capabilities`
/// field from `HelloMessage`/`DiscoveryBeacon` — the single-codebase
/// deployment doesn't need feature negotiation, and `ConfigMessage`
/// already carries every knob that actually matters.
pub const PROTOCOL_VERSION: u8 = 3;

/// Minimum supported protocol version. Equal to PROTOCOL_VERSION — no compat.
pub const MIN_PROTOCOL_VERSION: u8 = 3;

/// Default chunk size (1 MiB). Sized for QUIC, where the chunk is not
/// the ACK unit — retransmits happen at the packet layer regardless,
/// so the larger chunk just amortizes per-chunk overhead (one
/// unidirectional stream, one progress event, one SHA-256 segment).
pub const DEFAULT_CHUNK_SIZE: u32 = 1024 * 1024;

/// Default discovery port (UDP LAN beacons)
pub const DEFAULT_DISCOVERY_PORT: u16 = 14566;

/// Default transfer port (QUIC/UDP)
pub const DEFAULT_TRANSFER_PORT: u16 = 14567;

/// Default rendezvous server port (TCP control channel)
pub const DEFAULT_RENDEZVOUS_PORT: u16 = 14570;

/// Magic bytes for protocol framing
pub const PROTOCOL_MAGIC: [u8; 4] = *b"P2PF";

/// ALPN protocol name negotiated over QUIC's TLS 1.3 handshake.
pub const ALPN_PROTOCOL: &[u8] = b"p2pf/3";

// ─── 全局自定义设备名称 ───────────────────────────────────────────
//
// `mobile` 和 `hyx_isolates` 各自维护了一份 `CUSTOM_NAME`，但常驻的
// `DiscoveryManager`（由 `hyxStartListener` 创建）在启动时取一次名称就不再
// 更新。用户运行中改了自定义名称后，常驻 manager 的 beacon 仍携带旧名称。
//
// 这里在 core 层加一个全局名称：`create_beacon` 优先用它，这样所有活跃的
// `DiscoveryManager`（包括常驻的）都能即时反映新名称，无需重启。
// `mobile` / `hyx_isolates` 的 `set_device_name` 调 `core::set_device_name`
// 同步到此处。

use std::sync::{Mutex, OnceLock};

/// 全局自定义设备名称。`None` 时 `create_beacon` fallback 到构造时传入的名称。
static GLOBAL_CUSTOM_NAME: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn global_custom_name() -> &'static Mutex<Option<String>> {
    GLOBAL_CUSTOM_NAME.get_or_init(|| Mutex::new(None))
}

/// 设置全局自定义设备名称。空串（trim 后）视为重置为 `None`。
///
/// 由 `mobile::hyxSetDeviceName` 和 `hyx_isolates::device::set_device_name`
/// 调用。设置后，所有活跃的 `DiscoveryManager` 的 `create_beacon` 会即时
/// 拿到新名称，无需重启。
pub fn set_device_name(name: &str) {
    let trimmed = name.trim();
    let mut guard = global_custom_name().lock().expect("GLOBAL_CUSTOM_NAME lock");
    *guard = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
}

/// 返回全局自定义设备名称（如果已设置）。
///
/// `DiscoveryService::create_beacon` 优先用此值；`None` 时 fallback 到
/// 构造时传入的 `device_name`（默认 `hyx-{id前6位}`）。
pub fn try_custom_device_name() -> Option<String> {
    global_custom_name()
        .lock()
        .expect("GLOBAL_CUSTOM_NAME lock")
        .clone()
}

/// Normalize a user-supplied `host[:port]` string to one that always carries
/// a port, suitable for `tokio::net::lookup_host`. Handles IPv4 / IPv6 /
/// hostname forms, including bracketed and bare IPv6 literals — `contains(':')`
/// alone is not enough to tell whether an IPv6 string already has a port.
pub fn with_default_port(host_port: &str, default_port: u16) -> String {
    use std::net::{IpAddr, SocketAddr};
    if host_port.parse::<SocketAddr>().is_ok() {
        return host_port.to_string();
    }
    if let Ok(ip) = host_port.parse::<IpAddr>() {
        return SocketAddr::new(ip, default_port).to_string();
    }
    // Bare bracketed IPv6 without port, e.g. "[2001:db8::1]".
    if let Some(inner) = host_port
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
    {
        if let Ok(ip) = inner.parse::<IpAddr>() {
            return SocketAddr::new(ip, default_port).to_string();
        }
    }
    // Hostname form: only treat `host:port` as already-ported when the host
    // part has no remaining colons (rules out unbracketed IPv6 literals).
    if let Some((host, port)) = host_port.rsplit_once(':') {
        if !host.is_empty() && !host.contains(':') && port.parse::<u16>().is_ok() {
            return host_port.to_string();
        }
    }
    format!("{host_port}:{default_port}")
}

#[cfg(test)]
mod default_chunk_size_tests {
    use super::DEFAULT_CHUNK_SIZE;
    use crate::protocol::ConfigMessage;

    /// `DEFAULT_CHUNK_SIZE` is the single source of truth used on the wire
    /// (`ConfigMessage::default`), in CLI flags, and in GUI settings. Any
    /// future field that carries a default chunk size must also assert
    /// equality here so the three sides cannot silently drift.
    #[test]
    fn config_message_default_matches_default_chunk_size() {
        assert_eq!(ConfigMessage::default().chunk_size, DEFAULT_CHUNK_SIZE);
    }
}

#[cfg(test)]
mod with_default_port_tests {
    use super::with_default_port;

    #[test]
    fn ipv4_with_port_kept() {
        assert_eq!(with_default_port("1.2.3.4:80", 14570), "1.2.3.4:80");
    }

    #[test]
    fn ipv4_without_port_gets_default() {
        assert_eq!(with_default_port("1.2.3.4", 14570), "1.2.3.4:14570");
    }

    #[test]
    fn hostname_without_port_gets_default() {
        assert_eq!(with_default_port("example.com", 14570), "example.com:14570");
    }

    #[test]
    fn hostname_with_port_kept() {
        assert_eq!(
            with_default_port("example.com:9999", 14570),
            "example.com:9999"
        );
    }

    #[test]
    fn ipv6_bracketed_with_port_kept() {
        assert_eq!(
            with_default_port("[2001:db8::1]:9999", 14570),
            "[2001:db8::1]:9999"
        );
    }

    #[test]
    fn ipv6_bracketed_without_port_gets_default() {
        assert_eq!(
            with_default_port("[2001:db8::1]", 14570),
            "[2001:db8::1]:14570"
        );
    }

    #[test]
    fn ipv6_bare_gets_default() {
        assert_eq!(
            with_default_port("2001:db8::1", 14570),
            "[2001:db8::1]:14570"
        );
    }

    #[test]
    fn ipv6_loopback_bare_gets_default() {
        assert_eq!(with_default_port("::1", 14570), "[::1]:14570");
    }
}
