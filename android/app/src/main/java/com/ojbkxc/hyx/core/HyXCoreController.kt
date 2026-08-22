package com.ojbkxc.hyx.core

import android.content.ContentValues
import android.content.Context
import android.content.SharedPreferences
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

    private val pairAlphabet = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"

    init {
        loadSeedHistory()
        // Restore persisted devices (historical) before scanning so the 设备
        // tab shows both sections from the first frame.
        _devices.value = loadStoredDevices()
        startDiscovery()
    }

    /** Sender side of the simplest match: generate a fresh code, surface it as
     *  a QR for the peer to scan, then wait on the rendezvous server for them
     *  to dial in and receive [filePath]. */
    fun startPairing(filePath: String) {
        if (_status.value != TransferStatus.Idle) return
        val code = generatePairingCode()
        _pairingCode.value = PairingCode(code, System.currentTimeMillis() + PAIR_CODE_TTL_MS)
        _status.value = TransferStatus.Pairing
        if (!HyXNative.isLoaded) {
            nudgeStatusToTransferring()
            return
        }
        cancelled = false
        transferStartedMs = System.currentTimeMillis()
        val cfg = _settings.value
        _progress.value = TransferProgress(
            name = filePath.substringAfterLast('/').ifEmpty { "文件" },
            direction = _direction.value,
            transferredBytes = 0,
            totalBytes = 0,
            speedBps = 0.0,
            elapsedMs = 0
        )
        transferJob = viewModelScope.launch(Dispatchers.IO) {
            val err = HyXNative.hyxPairSend(
                code = code,
                serverAddress = RENDEZVOUS_SERVER,
                port = RENDEZVOUS_PORT,
                filePath = filePath,
                chunkBytes = 1_048_576,
                compression = if (cfg.compression) 1 else 0,
                aggregation = if (cfg.aggregation) 1 else 0,
                onProgress = ::recordProgress
            )
            if (cancelled) return@launch
            if (err.isNullOrEmpty()) markCompleted() else failTransfer(err)
        }
    }

    /** Receiver side: dial into the rendezvous room identified by [code] and
     *  receive into the staging dir. [code] may be a bare code, a `hyx://CODE`
     *  QR payload, or a `/CODE` path. */
    fun pairWithCode(code: String) {
        val clean = linkCode(code)
        if (clean.isEmpty()) return
        _pairingCode.value = PairingCode(clean, System.currentTimeMillis() + PAIR_CODE_TTL_MS)
        _status.value = TransferStatus.Pairing
        if (HyXNative.isLoaded) {
            cancelled = false
            transferStartedMs = System.currentTimeMillis()
            transferJob = viewModelScope.launch(Dispatchers.IO) {
                val err = HyXNative.hyxPairRendezvous(
                    code = clean,
                    serverAddress = RENDEZVOUS_SERVER,
                    port = RENDEZVOUS_PORT,
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

    /** Parse a QR payload (bare code, `hyx://CODE`, or `/CODE`) into a clean,
     *  uppercased pairing code. */
    private fun linkCode(raw: String): String =
        raw.substringAfterLast('/').trim().uppercase()

    fun scanQr(result: String) = pairWithCode(linkCode(result))

    private fun generatePairingCode(): String = buildString {
        val rand = java.util.Random()
        repeat(PAIR_CODE_LEN) {
            append(pairAlphabet[rand.nextInt(pairAlphabet.length)])
        }
    }

    companion object {
        const val RENDEZVOUS_SERVER = "rendezvous.hyx.dev:14567"
        const val RENDEZVOUS_PORT = 14567
        const val PAIR_CODE_LEN = 6
        const val PAIR_CODE_TTL_MS = 300_000L

        private const val DEV_STORE = "hyx_device_store"
        private const val DEV_KEY = "devices"
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

    fun cancelTransfer() {
        cancelled = true
        viewModelScope.launch(Dispatchers.IO) { HyXNative.hyxCancel() }
        _status.value = TransferStatus.Cancelled
        recordFinished(TransferStatus.Cancelled)
        transferJob?.cancel()
        _progress.value = null
        _pairingCode.value = null
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
            val nowOnline = raw.orEmpty()
                .lineSequence()
                .filter { it.isNotBlank() }
                .mapNotNull { parsePeerLine(it) }
                .toList()
            mergeDevices(nowOnline)
            _devicesScanning.value = false
        }
    }

    /** Parse one `name\tip:port\tdevice_id` discovery line into a [Device]. */
    private fun parsePeerLine(line: String): Device? {
        val parts = line.split('\t')
        if (parts.size < 3) return null
        val id = parts[2]
        if (id.isBlank()) return null
        return Device(
            id = id,
            name = parts[0],
            address = parts[1],
            via = Device.Via.Lan,
            online = true,
            allowTransfer = storedAllowTransfer(id)
        )
    }

    /** Online devices merge into the persisted list; peers not seen this scan
     *  drop out of 在线设备 and surface as 历史设备 (kept for deletion/拒收). */
    private fun mergeDevices(currentOnline: List<Device>) {
        val onlineIds = currentOnline.map { it.id }.toSet()
        val history = _devices.value
            .filter { !onlineIds.contains(it.id) }
            .map { it.copy(online = false) }

        val foldedOnline = currentOnline.map { od ->
            val known = (_devices.value + history).firstOrNull { it.id == od.id }
            if (known != null) od.copy(allowTransfer = known.allowTransfer) else od
        }
        _devices.value = foldedOnline + history
        persistDevices(_devices.value)
    }

    /** Flip the 接收/禁止 toggle for a device and persist the choice. */
    fun toggleAllowTransfer(id: String) {
        _devices.value = _devices.value.map {
            if (it.id == id) it.copy(allowTransfer = !it.allowTransfer) else it
        }
        persistDevices(_devices.value)
    }

    /** Forget a historical device (removes it from storage and the list). */
    fun removeHistoryDevice(id: String) {
        _devices.value = _devices.value.filterNot { it.id == id }
        persistDevices(_devices.value)
    }

    // ---------------------------------------------------------------------
    // Device persistence — known peers + their 接收/禁止 choice survive restarts.
    // The store is a single SharedPreferences string of newline-separated
    // "id\tname\taddress\tallow(1/0)" entries. Absent prefs (no appContext
    // yet) degrade to in-memory only.
    // ---------------------------------------------------------------------

    private fun devicePrefs(): SharedPreferences? =
        HyXNative.appContext?.getSharedPreferences(DEV_STORE, Context.MODE_PRIVATE)

    private fun loadStoredDevices(): List<Device> =
        devicePrefs()?.getString(DEV_KEY, null)
            ?.lineSequence()
            ?.mapNotNull { l ->
                val p = l.split('\t')
                if (p.size < 4) return@mapNotNull null
                Device(
                    id = p[0],
                    name = p[1],
                    address = p[2].ifEmpty { null },
                    via = Device.Via.Lan,
                    online = false,
                    allowTransfer = p[3] == "1"
                )
            }
            ?.toList()
            ?: emptyList()

    private fun storedAllowTransfer(id: String): Boolean {
        val entry = loadStoredDevices().firstOrNull { it.id == id } ?: return true
        return entry.allowTransfer
    }

    private fun persistDevices(devices: List<Device>) {
        val raw = devices.joinToString("\n") {
            "${it.id}\t${it.name}\t${it.address.orEmpty()}\t${if (it.allowTransfer) "1" else "0"}"
        }
        devicePrefs()?.edit()?.putString(DEV_KEY, raw)?.apply()
    }

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
     *  Idle reset the status sticks at Completed, and new transfers (which
     *  require Idle) would silently refuse to start. */
    private fun markCompleted() {
        _status.value = TransferStatus.Completed
        recordFinished(TransferStatus.Completed)
        _progress.value = null
        _pairingCode.value = null
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
        _pairingCode.value = null
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