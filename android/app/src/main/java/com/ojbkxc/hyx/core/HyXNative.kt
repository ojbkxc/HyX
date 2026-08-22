package com.ojbkxc.hyx.core

import android.content.Context
import android.util.Log
import java.io.File

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

    // @Volatile: written in ensureLoaded (usually main thread), read from
    // any thread via isLoaded — without this the store may not be visible.
    @Volatile
    private var loaded = false

    /** App-private file dir, set by HyXApp. Used as default receive landing zone. */
    @Volatile
    var appFilesDir: String = ""

    /** Application context, set by HyXApp; needed for MediaStore Downloads export. */
    @Volatile
    var appContext: Context? = null

    /** Private staging dir the Rust kernel writes received files into. */
    @Volatile
    var receiveDir: String = ""

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

    /** Bind + accept + receive the peer's files into [saveDir]. */
    external fun hyxStartListener(
        port: Int,
        chunkBytes: Int,
        fsyncEveryBytes: Long,
        compression: Int,
        aggregation: Int,
        saveDir: String,
        onProgress: ProgressCallback
    ): String?

    /** LAN-discover the peer, connect, then send [filePath]. */
    external fun hyxConnect(
        peerAddress: String,
        filePath: String,
        chunkBytes: Int,
        fsyncEveryBytes: Long,
        compression: Int,
        aggregation: Int,
        port: Int,
        onProgress: ProgressCallback
    ): String?

    /** Rendezvous pairing, then receive the peer's files into [saveDir]. */
    external fun hyxPairRendezvous(
        code: String,
        serverAddress: String,
        port: Int,
        compression: Int,
        saveDir: String,
        onProgress: ProgressCallback
    ): String?

    /** Rendezvous pairing, then send [filePath] to the paired peer. Sender
     *  side of a code/QR share. */
    external fun hyxPairSend(
        code: String,
        serverAddress: String,
        port: Int,
        filePath: String,
        chunkBytes: Int,
        compression: Int,
        aggregation: Int,
        onProgress: ProgressCallback
    ): String?

    external fun hyxCancel(): String?

    /** Real LAN discovery; returns newline-joined `"name\tip:port"` lines ("" if none found). */
    external fun hyxDiscover(port: Int): String?

    /**
     * Register a global Rust log callback. Must be called after [ensureLoaded]
     * (i.e. once `libhyx_mobile.so` is loaded) and before any other native call
     * that might emit a `tracing` event. Installs a process-global
     * `tracing-subscriber` Layer that forwards every Rust log event to [callback].
     */
    external fun hyxSetLogCallback(callback: LogCallback)

    fun interface ProgressCallback {
        /** Mirrors Rust-native progress: phase, bytes done, bytes total, speed B/s. */
        fun onProgress(phase: Int, transferred: Long, total: Long, speed: Long)
    }

    fun interface LogCallback {
        /**
         * Rust tracing log callback.
         * @param level   0=TRACE, 1=DEBUG, 2=INFO, 3=WARN, 4=ERROR
         * @param tag     tracing target (usually the module path, e.g. "hyx_mobile")
         * @param message formatted log message
         */
        fun onLog(level: Int, tag: String, message: String)
    }
}
