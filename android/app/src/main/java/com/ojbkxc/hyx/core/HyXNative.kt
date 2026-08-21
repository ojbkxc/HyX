package com.ojbkxc.hyx.core

import android.util.Log

/**
 * Thin JNI facade over the Rust `hyx-mobile` shared library.
 *
 * All native functions are `#[no_mangle] extern "C"` exports defined in
 * `mobile/src/lib.rs`, which delegate to the `hyx-core` transport kernel
 * while running on a Tokio runtime.
 *
 * Contracts:
 *  - `create_device` returns the hex cert fingerprint of this device, or an
 *    empty string on failure.
 *  - `start_listener`/`connect`/`pair_rendezvous`/`discover` return an error
 *    string ("" on success) and drive progress through `onProgress`.
 *  - `cancel` aborts the in-flight transfer.
 */
object HyXNative {
    private const val TAG = "HyXNative"

    private var loaded = false

    /** Must be called once, from HyXApp, before any other API. */
    fun ensureLoaded() {
        if (loaded) return
        try {
            System.loadLibrary("hyx_mobile")
            loaded = true
            Log.i(TAG, "hyx_mobile loaded")
        } catch (e: Throwable) {
            Log.e(TAG, "Failed to load hyx_mobile", e)
        }
    }

    val isLoaded: Boolean get() = loaded

    external fun hyxCreateDevice(selfCertFingerprint: String): String?

    external fun hyxStartListener(
        port: Int,
        chunkBytes: Int,
        fsyncEveryBytes: Long,
        compression: Int,
        aggregation: Int,
        onProgress: ProgressCallback
    ): String?

    external fun hyxConnect(
        peerAddress: String,
        chunkBytes: Int,
        fsyncEveryBytes: Long,
        compression: Int,
        aggregation: Int,
        onProgress: ProgressCallback
    ): String?

    external fun hyxPairRendezvous(
        code: String,
        serverAddress: String,
        onProgress: ProgressCallback
    ): String?

    external fun hyxCancel(): String?

    fun interface ProgressCallback {
        /** Mirrors Rust-native progress: phase, bytes done, bytes total, speed B/s. */
        fun onProgress(phase: Int, transferred: Long, total: Long, speed: Long)
    }
}