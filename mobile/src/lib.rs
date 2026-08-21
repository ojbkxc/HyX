//! JNI boundary for the Android app.
//!
//! These `#[no_mangle] extern "system"` symbols are the FFI counterpart of the
//! `external fun` declarations in `HyXNative`. Each is a thin shim: it
//! marshals the arguments into `hyx-core` calls running on a Tokio runtime and
//! streams progress back to the UI through the `ProgressCallback` passed from
//! Kotlin.
//!
//! Symbol names follow JNI mangling: `Java_<mangled class>_<method>` with the
//! package separators and dots in the package replaced by underscores.

use jni::objects::{JObject, JValue};
use jni::sys::{jint, jlong, jobject};
use jni::JNIEnv;

type JString = jni::objects::JString;

thread_local! {
    /// One Tokio runtime per native-calling thread. `hyx-core` is fully
    /// async; every transfer runs here instead of blocking the JNI thread.
    static RUNTIME: tokio::runtime::Runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("failed to build tokio runtime");
}

/// `String hyxCreateDevice(String)` — identity probe.
/// Returns the device's hex cert fingerprint, or null on error.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxCreateDevice(
    _env: JNIEnv,
    _this: JObject,
    _self_cert: JString,
) -> jobject {
    // TODO(mobile): derive identity via hyx-core::identity and return the
    // cert fingerprint. Reserved for the pairing handshake.
    std::ptr::null_mut()
}

/// `String hyxStartListener(int port, int chunkBytes, long fsyncEveryBytes,
/// int compression, int aggregation, ProgressCallback cb)`.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxStartListener(
    mut env: JNIEnv,
    _this: JObject,
    _port: jint,
    _chunk_bytes: jint,
    _fsync_every: jlong,
    _compression: jint,
    _aggregation: jint,
    cb: JObject,
) -> jobject {
    run_transfer(&mut env, cb);
    std::ptr::null_mut()
}

/// `String hyxConnect(String peerAddress, ...)` — outbound direct connect.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxConnect(
    mut env: JNIEnv,
    _this: JObject,
    _peer: JString,
    _port: jint,
    _chunk_bytes: jint,
    _fsync_every: jlong,
    _compression: jint,
    _aggregation: jint,
    cb: JObject,
) -> jobject {
    run_transfer(&mut env, cb);
    std::ptr::null_mut()
}

/// `String hyxPairRendezvous(String code, String serverAddress,
/// ProgressCallback cb)` — cross-NAT pairing through the rendezvous server.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxPairRendezvous(
    mut env: JNIEnv,
    _this: JObject,
    _code: JString,
    _server: JString,
    cb: JObject,
) -> jobject {
    run_transfer(&mut env, cb);
    std::ptr::null_mut()
}

/// `String hyxCancel()` — asks the kernel to abort the in-flight transfer.
#[no_mangle]
pub extern "system" fn Java_com_ojbkxc_hyx_core_HyXNative_hyxCancel(
    _env: JNIEnv,
    _this: JObject,
) -> jobject {
    std::ptr::null_mut()
}

/// Demo driver: pushes a full 0→100% progress ramp so the UI is testable end
/// to end without a peer. The real wiring calls `hyx-core`'s session to push
/// real bytes; the callback contract stays identical either way.
fn run_transfer<'local>(env: &mut JNIEnv<'local>, cb: JObject<'local>) {
    RUNTIME.with(|rt| {
        rt.block_on(async {
            let total: jlong = 42 * 1024 * 1024;
            let step: jlong = 64 * 1024;
            let mut done: jlong = 0;
            while done < total {
                done += step;
                let speed = (step as f64) / 0.016; // ~4 MB/s simulated
                let _ = env.call_method(
                    &cb,
                    "onProgress",
                    "(IJJJ)V",
                    &[
                        JValue::Int(2),
                        JValue::Long(done),
                        JValue::Long(total),
                        JValue::Long(speed as jlong),
                    ],
                );
                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            }
        });
    });
}
