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

    /**
     * LAN 直连发送（带缓存 fingerprint 的 TOFU/pin 连接），对齐 Flutter app 的
     * `transfer.rs::connect`。决策树：
     *  - [peerAddress] 非空 + [cachedFingerprint] 非空有效 hex → 直接 pin 连接（快路径）。
     *  - [peerAddress] 非空 + [cachedFingerprint] 空/无效 → 发现拿 fp，失败回退 TOFU。
     *  - [peerAddress] 空 → 自动发现一个 peer 后连接（原行为）。
     *
     * TOFU 路径成功后，Rust 侧通过 [ProgressCallback.onPeerFingerprint] 回传对端
     * fingerprint（hex），Kotlin 侧应缓存到对应 Device 以便下次 pin 连接。
     * pin 路径不回传（调用方已有该 fp）。
     *
     * 注意：参数列表与 [hyxConnect] 不同——Rust 侧精简掉了 fsyncEveryBytes 和
     * aggregation（详见 mobile/src/lib.rs 的 Java_com_ojbkxc_hyx_core_HyXNative_hyxConnectWithFp）。
     */
    external fun hyxConnectWithFp(
        peerAddress: String,
        filePath: String,
        chunkBytes: Int,
        compression: Int,
        port: Int,
        cachedFingerprint: String,
        onProgress: ProgressCallback
    ): String?


    external fun hyxCancel(): String?

    /** Real LAN discovery; returns newline-joined `"name\tip:port"` lines ("" if none found). */
    external fun hyxDiscover(port: Int): String?

    /**
     * 对指定 IP 单播探测对端是否在线（跨子网发现）。在线返回 `"name\tip:port\tdevice_id"`，
     * 离线/失败返回空串。由蓝牙层在读到邻居候选 IP 后调用，以决定其在线状态。
     */
    external fun hyxProbeIp(ip: String, port: Int): String?

    /** 设置自定义设备名称（空串重置为默认名）。 */
    external fun hyxSetDeviceName(name: String)

    /**
     * Register a global Rust log callback. Must be called after [ensureLoaded]
     * (i.e. once `libhyx_mobile.so` is loaded) and before any other native call
     * that might emit a `tracing` event. Installs a process-global
     * `tracing-subscriber` Layer that forwards every Rust log event to [callback].
     */
    external fun hyxSetLogCallback(callback: LogCallback)

    /**
     * JNI 进度回调。改为普通 interface（非 fun interface）以容纳第二个方法
     * [onPeerFingerprint]——Rust 侧 drain 在同一个 cb 对象上调用 onPeerFingerprint
     * （见 mobile/src/lib.rs 的 Evt::PeerFingerprint 分支），所以不能拆成两个
     * fun interface。
     *
     * 默认实现的 [onPeerFingerprint] 保证现有只关心 onProgress 的调用方不破坏；
     * 但 `::recordProgress` 这类方法引用不再能自动转换为 ProgressCallback，
     * 调用方需改用 object 表达式或共享的 callback 字段。
     */
    interface ProgressCallback {
        /** Mirrors Rust-native progress: phase, bytes done, bytes total, speed B/s. */
        fun onProgress(phase: Int, transferred: Long, total: Long, speed: Long)

        /**
         * TOFU 连接成功后 Rust 侧回传对端证书 fingerprint（hex 字符串）。
         * Kotlin 侧应在此缓存指纹到对应 Device，下次连接可直接 pin 跳过 UDP 发现。
         * 默认空实现：不关心 fingerprint 的调用方（如 hyxStartListener）无需关心。
         */
        fun onPeerFingerprint(fingerprint: String) {}
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
