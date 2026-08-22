//! Device identity — mirrors `mobile/src/lib.rs`'s once-per-process identity,
//! exposing the stable id and cert fingerprint to Dart.

use crate::state;

/// Load/generate the device identity and return its hex cert fingerprint.
#[flutter_rust_bridge::frb(sync)]
pub fn create_device() -> Option<String> {
    let fp = state::identity().fingerprint_hex();
    (!fp.is_empty()).then_some(fp)
}

/// Stable per-device id (dedup key across LAN, mirrors beacons).
#[flutter_rust_bridge::frb(sync)]
pub fn self_device_id() -> String {
    state::device_id().to_string()
}

/// Human-readable device name broadcast in discovery beacons.
#[flutter_rust_bridge::frb(sync)]
pub fn device_name() -> String {
    state::device_name()
}