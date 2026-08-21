package com.ojbkxc.hyx

import android.app.Application
import com.ojbkxc.hyx.core.HyXNative
import java.io.File

class HyXApp : Application() {
    override fun onCreate() {
        super.onCreate()
        val incoming = File(filesDir, "incoming").apply { mkdirs() }
        HyXNative.appContext = this
        HyXNative.appFilesDir = filesDir.absolutePath
        HyXNative.receiveDir = incoming.absolutePath
        HyXNative.ensureLoaded()
    }
}