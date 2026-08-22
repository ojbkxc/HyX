//! Process-global state reused by the FRB api modules: a shared Tokio runtime,
//! the long-lived device identity, and the stable per-device id. Kept OUTSIDE
//! `api/` so flutter_rust_bridge never sees these internal types.

use std::sync::{Arc, OnceLock};

use hyx_core::identity::{device_id_from_fingerprint, Identity};
use hyx_core::Uuid;
use tokio::runtime::{Builder, Runtime};

/// One multi-threaded Tokio runtime shared by every transfer. `enable_all`
/// brings the IO + time drivers that QUIC / rendezvous need.
pub(crate) fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Load or generate the device identity (persisted by hyx-core).
pub(crate) fn identity() -> Arc<Identity> {
    static ID: OnceLock<Arc<Identity>> = OnceLock::new();
    ID.get_or_init(|| {
        Arc::new(
            Identity::load_or_generate(None)
                .unwrap_or_else(|_| Identity::generate().expect("generate identity")),
        )
    })
    .clone()
}

/// Stable per-device id derived from the cert fingerprint, so a peer's
/// identity survives app restarts (mirrors `mobile/src/lib.rs`).
pub(crate) fn device_id() -> Uuid {
    static DID: OnceLock<Uuid> = OnceLock::new();
    *DID.get_or_init(|| device_id_from_fingerprint(&identity().fingerprint()))
}

pub(crate) fn device_name() -> String {
    format!("hyx-{}", &device_id().to_string()[..6])
}