package com.ojbkxc.hyx.core

import com.ojbkxc.hyx.ui.model.LogEntry
import com.ojbkxc.hyx.ui.model.LogLevel
import com.ojbkxc.hyx.ui.model.LogSource
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * Process-wide log buffer. Collects two sources into a single ring buffer:
 *  - Rust kernel — via [onRustLog], invoked through the JNI `LogCallback` set
 *    in `HyXApp.onCreate`.
 *  - Android Kotlin — via [add], called by [HyXLog].
 *
 * Exposes a [StateFlow] so Compose UI can reactively render the log panel.
 * The buffer is bounded at [MAX_ENTRIES] to keep memory flat under high log
 * volume (older entries are dropped first).
 */
object LogCollector {
    private const val MAX_ENTRIES = 2000

    private val _logs = MutableStateFlow<List<LogEntry>>(emptyList())

    /** Snapshot stream of collected logs; always holds the most recent ≤2000 entries. */
    val logs: StateFlow<List<LogEntry>> = _logs.asStateFlow()

    /** Append [entry]; if the buffer is full the oldest entry is evicted. */
    fun add(entry: LogEntry) {
        android.util.Log.d("R", "LogCollector.add")
        _logs.update { (it + entry).takeLast(MAX_ENTRIES) }
    }

    /** Drop every collected entry. */
    fun clear() {
        _logs.value = emptyList()
    }

    /** All entries joined with newlines, each in [LogEntry.formatted] form. */
    fun export(): String = _logs.value.joinToString("\n") { it.formatted() }

    /**
     * Rust JNI callback entry point. Signature matches [HyXNative.LogCallback.onLog]
     * so `LogCollector::onRustLog` can be passed directly to `hyxSetLogCallback`.
     */
    fun onRustLog(level: Int, tag: String, message: String) {
        android.util.Log.d("R", "LogCollector.onRustLog")
        add(
            LogEntry(
                timestamp = System.currentTimeMillis(),
                level = LogLevel.fromOrdinal(level),
                source = LogSource.Rust,
                tag = tag,
                message = message
            )
        )
    }
}