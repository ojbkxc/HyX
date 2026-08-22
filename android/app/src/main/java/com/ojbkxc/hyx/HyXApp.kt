package com.ojbkxc.hyx

import android.app.Application
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
        HyXLog.i("HyXApp", "Application initialized, native loaded=${HyXNative.isLoaded}")
    }
}
