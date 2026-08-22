package com.ojbkxc.hyx.core

import android.util.Log
import com.ojbkxc.hyx.ui.model.LogEntry
import com.ojbkxc.hyx.ui.model.LogLevel
import com.ojbkxc.hyx.ui.model.LogSource

/**
 * Android-side log wrapper. Every call goes to two sinks:
 *  1. `android.util.Log` — visible in logcat for `adb logcat` debugging.
 *  2. [LogCollector] — surfaced in the in-app log panel UI.
 *
 * Use this instead of bare `android.util.Log` so logs are collected for the
 * transfer-page log viewer. The Rust kernel does not go through here; it
 * arrives via the JNI `LogCallback` → `LogCollector.onRustLog`.
 */
object HyXLog {
    fun i(tag: String, msg: String) = log(LogLevel.Info, tag, msg)

    fun w(tag: String, msg: String) = log(LogLevel.Warn, tag, msg)

    fun e(tag: String, msg: String, t: Throwable? = null) = log(LogLevel.Error, tag, msg, t)

    fun d(tag: String, msg: String) = log(LogLevel.Debug, tag, msg)

    private fun log(level: LogLevel, tag: String, msg: String, t: Throwable? = null) {
        // 1. logcat (adb visible)
        when (level) {
            LogLevel.Info -> Log.i(tag, msg)
            LogLevel.Warn -> Log.w(tag, msg)
            LogLevel.Error -> Log.e(tag, msg, t)
            LogLevel.Debug -> Log.d(tag, msg)
            LogLevel.Trace -> Log.v(tag, msg)
        }
        // 2. in-app collector
        LogCollector.add(
            LogEntry(
                timestamp = System.currentTimeMillis(),
                level = level,
                source = LogSource.Android,
                tag = tag,
                message = if (t != null) "$msg\n${t.stackTraceToString()}" else msg
            )
        )
    }
}