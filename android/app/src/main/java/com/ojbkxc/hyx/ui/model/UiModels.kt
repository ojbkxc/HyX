package com.ojbkxc.hyx.ui.model

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Transfer direction, mirrored from hyx-core. */
enum class TransferDirection { Send, Receive }

/** Terminal transfer state, mirrored from hyx-core history::TransferStatus. */
enum class TransferStatus { Idle, Pairing, Connecting, Transferring, Completed, Failed, Cancelled }

/** A short pairing code for rendezvous (cross-NAT). */
data class PairingCode(val code: String, val expiresAtMs: Long)

/**
 * A peer reachable on the LAN (beacon discovery) or via rendezvous.
 * @param id             stable per-device UUID carried in the discovery beacon;
 *                       keys dedup — the same phone seen on different subnets is one entry.
 * @param online         true when currently discovered on the LAN, false for a historical peer.
 * @param allowTransfer  whether transfers from this device are accepted (接收/禁止).
 */
data class Device(
    val id: String,
    val name: String,
    val address: String?,
    val via: Via,
    val online: Boolean,
    val allowTransfer: Boolean = true
) {
    enum class Via { Lan, Rendezvous }
}

/** Snapshot of an active transfer for the dashboard cards. */
data class TransferProgress(
    val name: String,
    val direction: TransferDirection,
    val transferredBytes: Long,
    val totalBytes: Long,
    val speedBps: Double,
    val elapsedMs: Long
) {
    val fraction: Float
        get() = if (totalBytes > 0) (transferredBytes.toFloat() / totalBytes).coerceIn(0f, 1f) else 0f
}

/** Structured record, mirrors hyx-core history::TransferRecord. */
data class HistoryRecord(
    val id: String,
    val name: String,
    val direction: TransferDirection,
    val status: TransferStatus,
    val bytesTransferred: Long,
    val peerAddress: String,
    val durationSecs: Long,
    val timestamp: Long
)

/** The three engine knobs (fsync / compression / aggregation) surfaced as config. */
data class EngineSettings(
    val fsyncEveryBytes: Long = 8L * 1024 * 1024, // 引擎A：rsync 聚合后的 fsync 频率
    val compression: Boolean = true,
    val aggregation: Boolean = true // 引擎B：单流多帧聚合
)

/** Log severity, ordinal-aligned with Rust tracing_core::Level (0=TRACE … 4=ERROR). */
enum class LogLevel {
    Trace, Debug, Info, Warn, Error;

    companion object {
        /** Map a Rust-side level int to [LogLevel]; out-of-range falls back to [Info]. */
        fun fromOrdinal(n: Int): LogLevel = entries.getOrElse(n) { Info }
    }
}

/** Where a log entry originated: Rust kernel via JNI callback, or Android Kotlin. */
enum class LogSource { Rust, Android }

/**
 * A single log record collected by [com.ojbkxc.hyx.core.LogCollector].
 * @param timestamp wall-clock millis (System.currentTimeMillis)
 * @param level     severity
 * @param source    Rust or Android
 * @param tag       tracing target / Android log tag
 * @param message   formatted message
 */
data class LogEntry(
    val timestamp: Long,
    val level: LogLevel,
    val source: LogSource,
    val tag: String,
    val message: String
) {
    /** "HH:mm:ss.SSS [LEVEL] [Source] tag: message" — one line, export-friendly. */
    fun formatted(): String {
        // SimpleDateFormat is not thread-safe; create per call (logs are bounded at 2000).
        val time = SimpleDateFormat("HH:mm:ss.SSS", Locale.US).format(Date(timestamp))
        return "$time [${level.name.uppercase()}] [${source.name}] $tag: $message"
    }
}