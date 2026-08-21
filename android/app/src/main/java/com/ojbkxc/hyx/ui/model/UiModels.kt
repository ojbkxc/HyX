package com.ojbkxc.hyx.ui.model

/** Transfer direction, mirrored from hyx-core. */
enum class TransferDirection { Send, Receive }

/** Terminal transfer state, mirrored from hyx-core history::TransferStatus. */
enum class TransferStatus { Idle, Pairing, Connecting, Transferring, Completed, Failed, Cancelled }

/** A short pairing code for rendezvous (cross-NAT). */
data class PairingCode(val code: String, val expiresAtMs: Long)

/** A peer reachable on the LAN (beacon discovery) or via rendezvous. */
data class Device(
    val id: String,
    val name: String,
    val address: String?,
    val via: Via,
    val connected: Boolean
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