package com.ojbkxc.hyx.core

import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ojbkxc.hyx.ui.components.mimeTypeOf
import com.ojbkxc.hyx.ui.model.Device
import com.ojbkxc.hyx.ui.model.EngineSettings
import com.ojbkxc.hyx.ui.model.HistoryRecord
import com.ojbkxc.hyx.ui.model.PairingCode
import com.ojbkxc.hyx.ui.model.TransferDirection
import com.ojbkxc.hyx.ui.model.TransferProgress
import com.ojbkxc.hyx.ui.model.TransferStatus
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * Single state holder for the whole app. Owns the conversation with the Rust
 * kernel through [HyXNative]; exposes optimistic UI state so the three tabs
 * (传输/设备/历史) all read from one source of truth.
 */
class HyXCoreController : ViewModel() {

    private val _status = MutableStateFlow(TransferStatus.Idle)
    val status: StateFlow<TransferStatus> = _status.asStateFlow()

    private val _direction = MutableStateFlow(TransferDirection.Send)
    val direction: StateFlow<TransferDirection> = _direction.asStateFlow()

    private val _devices = MutableStateFlow<List<Device>>(emptyList())
    val devices: StateFlow<List<Device>> = _devices.asStateFlow()

    private val _devicesScanning = MutableStateFlow(false)
    val devicesScanning: StateFlow<Boolean> = _devicesScanning.asStateFlow()

    private val _pairingCode = MutableStateFlow<PairingCode?>(null)
    val pairingCode: StateFlow<PairingCode?> = _pairingCode.asStateFlow()

    private val _progress = MutableStateFlow<TransferProgress?>(null)
    val progress: StateFlow<TransferProgress?> = _progress.asStateFlow()

    private val _history = MutableStateFlow<List<HistoryRecord>>(emptyList())
    val history: StateFlow<List<HistoryRecord>> = _history.asStateFlow()

    private val _settings = MutableStateFlow(EngineSettings())
    val settings: StateFlow<EngineSettings> = _settings.asStateFlow()

    // Speed of progress updates is throttled by the Rust side; no EMA here yet.
    private var transferJob: Job? = null
    private var transferStartedMs = 0L
    // Set by cancelTransfer so the still-running native call's return value
    // (which surfaces as "Transfer cancelled") doesn't double-record a failure.
    // @Volatile: written from the UI thread, read from Dispatchers.IO coroutines.
    @Volatile
    private var cancelled = false

    init {
        loadSeedHistory()
        startDiscovery()
    }

    fun onDirectionChange(d: TransferDirection) {
        _direction.value = d
    }

    fun updateSettings(transform: (EngineSettings) -> EngineSettings) {
        _settings.value = transform(_settings.value)
    }

    fun selectFiles(names: List<String>) {
        _progress.value = TransferProgress(
            name = names.take(1).firstOrNull() ?: "文件",
            direction = _direction.value,
            transferredBytes = 0,
            totalBytes = 0,
            speedBps = 0.0,
            elapsedMs = 0
        )
    }

    fun startTransfer() {
        if (_status.value == TransferStatus.Transferring) return
        // Send mode picks a file via the system picker; only Receive mode
        // has a "start listener" action here.
        if (_direction.value == TransferDirection.Send) return
        cancelled = false
        _status.value = TransferStatus.Connecting
        transferStartedMs = System.currentTimeMillis()
        val cfg = _settings.value
        if (HyXNative.isLoaded) {
            transferJob = viewModelScope.launch(Dispatchers.IO) {
                val err = HyXNative.hyxStartListener(
                    port = 14567,
                    chunkBytes = 1_048_576,
                    fsyncEveryBytes = cfg.fsyncEveryBytes,
                    compression = if (cfg.compression) 1 else 0,
                    aggregation = if (cfg.aggregation) 1 else 0,
                    saveDir = HyXNative.receiveDir,
                    onProgress = ::recordProgress
                )
                if (cancelled) return@launch
                if (err.isNullOrEmpty()) {
                    markCompleted()
                    exportReceivedToDownloads()
                } else {
                    failTransfer(err)
                }
            }
        } else {
            // No library: simulate a transfer so the UI stays demonstrable.
            simulateTransfer()
        }
    }

    /** Send one file to a discovered peer (sender side of the link). */
    fun sendFileToPeer(peerAddress: String, filePath: String) {
        // Only start from Idle: a send during Connecting/Pairing/Transferring
        // would race the in-flight native call and corrupt its state.
        if (_status.value != TransferStatus.Idle) return
        if (!HyXNative.isLoaded) {
            simulateTransfer()
            return
        }
        cancelled = false
        val cfg = _settings.value
        _status.value = TransferStatus.Connecting
        transferStartedMs = System.currentTimeMillis()
        _progress.value = TransferProgress(
            name = filePath.substringAfterLast('/').ifEmpty { "文件" },
            direction = _direction.value,
            transferredBytes = 0,
            totalBytes = 0,
            speedBps = 0.0,
            elapsedMs = 0
        )
        transferJob = viewModelScope.launch(Dispatchers.IO) {
            val err = HyXNative.hyxConnect(
                peerAddress = peerAddress,
                filePath = filePath,
                chunkBytes = 1_048_576,
                fsyncEveryBytes = cfg.fsyncEveryBytes,
                compression = if (cfg.compression) 1 else 0,
                aggregation = if (cfg.aggregation) 1 else 0,
                port = 14567,
                onProgress = ::recordProgress
            )
            if (cancelled) return@launch
            if (err.isNullOrEmpty()) markCompleted() else failTransfer(err)
        }
    }

    fun pairWithCode(code: String) {
        val server = "rendezvous.hyx.dev:14567"
        _pairingCode.value = PairingCode(code, System.currentTimeMillis() + 300_000L)
        _status.value = TransferStatus.Pairing
        if (HyXNative.isLoaded) {
            cancelled = false
            transferStartedMs = System.currentTimeMillis()
            transferJob = viewModelScope.launch(Dispatchers.IO) {
                val err = HyXNative.hyxPairRendezvous(
                    code = code,
                    serverAddress = server,
                    port = 14567,
                    compression = if (_settings.value.compression) 1 else 0,
                    saveDir = HyXNative.receiveDir,
                    onProgress = ::recordProgress
                )
                if (cancelled) return@launch
                if (err.isNullOrEmpty()) {
                    markCompleted()
                    exportReceivedToDownloads()
                } else {
                    failTransfer(err)
                }
            }
        } else {
            nudgeStatusToTransferring()
        }
    }

    fun scanQr(result: String) {
        val code = result.substringAfterLast('/').ifEmpty { result }
        pairWithCode(code)
    }

    fun cancelTransfer() {
        cancelled = true
        viewModelScope.launch(Dispatchers.IO) { HyXNative.hyxCancel() }
        _status.value = TransferStatus.Cancelled
        recordFinished(TransferStatus.Cancelled)
        transferJob?.cancel()
        _progress.value = null
        cleanupSendCache()
        // Hold the Cancelled status briefly before returning to Idle, mirroring
        // failTransfer's 1.5 s hold so the user actually sees the cancellation.
        viewModelScope.launch {
            delay(1500)
            _status.value = TransferStatus.Idle
        }
    }

    fun startDiscovery() {
        _devicesScanning.value = true
        viewModelScope.launch(Dispatchers.IO) {
            val raw = if (HyXNative.isLoaded) HyXNative.hyxDiscover(14567) else null
            val found = raw.orEmpty()
                .lineSequence()
                .filter { it.isNotBlank() }
                .mapNotNull { line ->
                    val tab = line.indexOf('\t')
                    if (tab < 0) return@mapNotNull null
                    val name = line.substring(0, tab)
                    val addr = line.substring(tab + 1)
                    Device(
                        id = addr,
                        name = name,
                        address = addr,
                        via = Device.Via.Lan,
                        connected = false
                    )
                }
                .toList()
            _devices.value = found
            _devicesScanning.value = false
        }
    }

    /** Pick the preferred target (a connected peer first, else any discovered one). */
    fun targetPeerAddress(): String =
        _devices.value.firstOrNull { it.connected }?.address
            ?: _devices.value.firstOrNull()?.address
            ?: ""

    /** Send [filePath] to the best-known peer (real native transfer). */
    fun sendPickedFile(filePath: String) = sendFileToPeer(targetPeerAddress(), filePath)

    /**
     * Move received files from the private staging dir into the system Downloads
     * collection via MediaStore (API 29+) or the public Downloads folder (< API 29).
     * Only files that were actually exported are removed from staging — a failed
     * export keeps its source so the user doesn't lose data.
     */
    private fun exportReceivedToDownloads() {
        val ctx = HyXNative.appContext ?: return
        val staging = File(HyXNative.receiveDir.ifEmpty { return })
        if (!staging.exists()) return
        val files = staging.listFiles()?.filter { it.isFile } ?: return
        files.forEach { f ->
            val ok = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                insertIntoMediaStore(ctx, f)
            } else {
                legacyCopyToDownloads(f)
            }
            if (ok) runCatching { f.delete() }
        }
    }

    private fun insertIntoMediaStore(ctx: Context, f: File): Boolean {
        val resolver = ctx.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, f.name)
            put(MediaStore.MediaColumns.MIME_TYPE, mimeTypeOf(f.name))
            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: return false
        return try {
            resolver.openOutputStream(uri)?.use { out ->
                f.inputStream().use { it.copyTo(out) }
            } ?: return false
            values.clear()
            values.put(MediaStore.MediaColumns.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            true
        } catch (e: Exception) {
            runCatching { resolver.delete(uri, null, null) }
            false
        }
    }

    private fun legacyCopyToDownloads(f: File): Boolean = runCatching {
        val destDir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
        if (!destDir.exists()) destDir.mkdirs()
        val dest = File(destDir, f.name)
        f.inputStream().use { i -> dest.outputStream().use { o -> i.copyTo(o) } }
        true
    }.getOrDefault(false)

    fun pingPeer(id: String) {
        // Toggle the "connected" marker used to prefer a peer for sending.
        // No transfer is started here — the actual send happens via
        // sendPickedFile from the transfer tab.
        _devices.value = _devices.value.map {
            if (it.id == id) it.copy(connected = !it.connected) else it
        }
    }

    private fun nudgeStatusToTransferring() {
        viewModelScope.launch {
            delay(500)
            if (_status.value == TransferStatus.Connecting || _status.value == TransferStatus.Pairing) {
                _status.value = TransferStatus.Transferring
                if (!HyXNative.isLoaded) simulateTransfer()
            }
        }
    }

    private fun recordProgress(phase: Int, transferred: Long, total: Long, speed: Long) {
        if (cancelled) return
        val elapsed = System.currentTimeMillis() - transferStartedMs
        _status.value = TransferStatus.Transferring
        _progress.value = TransferProgress(
            name = _progress.value?.name ?: "文件",
            direction = _direction.value,
            transferredBytes = transferred,
            totalBytes = total,
            speedBps = speed.toDouble(),
            elapsedMs = elapsed
        )
        // Completion is decided by the native call's return value (the JNI
        // drain loop exits with a Done event after the transfer finishes),
        // not by transferred >= total here — a final callback that lands a
        // few bytes short of total would otherwise never mark the transfer
        // done, and firing here would double-record history.
    }

    /** Terminal success: record history once, clear the progress panel, then
     *  return to Idle so the user can start the next transfer. Without the
     *  Idle reset the status sticks at Completed and sendFileToPeer (which
     *  requires Idle) silently refuses every later send. */
    private fun markCompleted() {
        _status.value = TransferStatus.Completed
        recordFinished(TransferStatus.Completed)
        _progress.value = null
        cleanupSendCache()
        viewModelScope.launch {
            delay(1500)
            _status.value = TransferStatus.Idle
        }
    }

    /** Standalone demo path for when libhyx_mobile.so isn't built. */
    private fun simulateTransfer() {
        val total = 42L * 1024 * 1024
        transferJob = viewModelScope.launch {
            var done = 0L
            while (done < total) {
                done += 64L * 1024
                recordProgress(2, done, total, 4194304)
                delay(16)
            }
            if (!cancelled) markCompleted()
        }
    }

    private fun recordFinished(status: TransferStatus) {
        _history.value = listOf(
            HistoryRecord(
                id = System.nanoTime().toString(),
                name = _progress.value?.name ?: "文件",
                direction = _direction.value,
                status = status,
                bytesTransferred = _progress.value?.transferredBytes ?: 0L,
                peerAddress = _devices.value.firstOrNull()?.address ?: "局域网",
                durationSecs = ((System.currentTimeMillis() - transferStartedMs) / 1000).coerceAtLeast(0),
                timestamp = System.currentTimeMillis()
            )
        ) + _history.value
    }

    private fun failTransfer(msg: String) {
        _status.value = TransferStatus.Failed
        recordFinished(TransferStatus.Failed)
        _progress.value = null
        cleanupSendCache()
        viewModelScope.launch {
            delay(1500)
            _status.value = TransferStatus.Idle
        }
    }

    /**
     * Delete staged send copies in the app cache (created by TransferScreen's
     * copyToCache). Without this the cache grows unbounded across sends.
     */
    private fun cleanupSendCache() {
        val ctx = HyXNative.appContext ?: return
        File(ctx.cacheDir, "hyx_send").listFiles()?.forEach { runCatching { it.delete() } }
    }

    private fun loadSeedHistory() {
        _history.value = listOf(
            HistoryRecord("h1", "设计稿.zip", TransferDirection.Send, TransferStatus.Completed, 2048L * 1024 * 1024, "192.168.1.44", 18, System.currentTimeMillis()),
            HistoryRecord("h2", "演唱会视频.mp4", TransferDirection.Receive, TransferStatus.Completed, 6L * 1024 * 1024 * 1024, "192.168.1.98", 340, System.currentTimeMillis() - 3_600_000L),
            HistoryRecord("h3", "全家福", TransferDirection.Send, TransferStatus.Failed, 96L * 1024 * 1024, "对端手机", 7, System.currentTimeMillis() - 86_400_000L)
        )
    }

    override fun onCleared() {
        transferJob?.cancel()
        if (_status.value in setOf(TransferStatus.Connecting, TransferStatus.Pairing, TransferStatus.Transferring)) {
            // cancel() only aborts the coroutine; a transfer currently blocked
            // in a native call keeps running until it finishes. Abort the
            // native side too so no background receive continues after the VM dies.
            HyXNative.hyxCancel()
        }
        super.onCleared()
    }
}