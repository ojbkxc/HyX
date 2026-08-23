package com.ojbkxc.hyx

import android.app.Application
import android.content.Context
import android.net.wifi.WifiManager
import com.ojbkxc.hyx.core.HyXLog
import com.ojbkxc.hyx.core.HyXNative
import com.ojbkxc.hyx.core.LogCollector
import java.io.File

class HyXApp : Application() {
    override fun onCreate() {
        super.onCreate()
        val incoming = File(filesDir, "incoming").apply { mkdirs() }
        HyXNative.appContext = this
        HyXNative.appFilesDir = filesDir.absolutePath
        HyXNative.receiveDir = incoming.absolutePath
        HyXNative.ensureLoaded()
        // Wire Rust tracing → LogCollector so kernel logs show in the log panel.
        // Must run after ensureLoaded (libhyx_mobile.so is loaded) so the JNI
        // symbol `hyxSetLogCallback` is resolvable.
        if (HyXNative.isLoaded) {
            HyXNative.hyxSetLogCallback(LogCollector::onRustLog)
        }

        // 持有 WifiManager.MulticastLock — Android WiFi 芯片默认在省电模式下
        // 过滤掉多播包，不持有这个锁的话 join_multicast_v4 也收不到别人的 beacon。
        // 锁在 app 整个生命周期内持有，app 退出时系统自动释放。
        acquireMulticastLock()

        HyXLog.i("HyXApp", "Application initialized, native loaded=${HyXNative.isLoaded}")
    }

    /**
     * 获取并永久持有 WiFi 多播锁，让 UDP 多播发现包能被 WiFi 芯片接收。
     *
     * 没有 MulticastLock 时，Android 设备即使 join 了多播组也收不到多播包
     * （WiFi 驱动为省电过滤掉非单播流量）。这是 localsend 等局域网发现 app
     * 在 Android 上的必备操作。
     */
    private fun acquireMulticastLock() {
        try {
            val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
            val lock = wifiManager.createMulticastLock("HyXDiscovery")
            lock.setReferenceCounted(false)
            lock.acquire()
            HyXLog.i("HyXApp", "MulticastLock acquired — UDP multicast discovery enabled")
        } catch (e: Exception) {
            HyXLog.w("HyXApp", "Failed to acquire MulticastLock: ${e.message}")
        }
    }
}
