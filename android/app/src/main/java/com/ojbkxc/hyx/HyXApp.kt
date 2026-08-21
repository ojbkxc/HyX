package com.ojbkxc.hyx

import android.app.Application
import com.ojbkxc.hyx.core.HyXNative

class HyXApp : Application() {
    override fun onCreate() {
        super.onCreate()
        HyXNative.ensureLoaded()
    }
}