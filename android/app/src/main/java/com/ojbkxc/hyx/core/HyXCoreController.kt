package com.ojbkxc.hyx.core

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ojbkxc.hyx.ui.model.Device
import com.ojbkxc.hyx.ui.model.EngineSettings
import com.ojbkxc.hyx.ui.model.HistoryRecord
import com.ojbkxc.hyx.ui.model.PairingCode
import com.ojbkxc.hyx.ui.model.TransferDirection
import com.ojbkxc.hyx.ui.model.TransferProgress
import com.ojbkxc.hyx.ui.model.TransferStatus
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
                    onProgress = ::recordProgress
                )
                if (err?.isNotEmpty() == true) failTransfer(err.toString())
            }
        } else {
            simulateTransfer()
        }
        // Mock device appears on the LAN once we're listening; real beacon
        // discovery replaces this once hyx-core discovery lands on Android.
        fakePeer("对端手机", "192.168.1.66")
    }

    fun pairWithCode(code: String) {
        val server = "rendezvous.hyx.dev:14567"
        _pairingCode.value = PairingCode(code, System.currentTimeMillis() + 300_000L)
        _status.value = TransferStatus.Pairing
        nudgeStatusToTransferring()
        fakePeer("对端手机", null)
    }

    fun scanQr(result: String) {
        val code = result.substringAfterLast('/').ifEmpty { result }
        pairWithCode(code)
    }

    fun cancelTransfer() {
        viewModelScope.launch(Dispatchers.IO) { HyXNative.hyxCancel() }
        _status.value = TransferStatus.Cancelled
        recordFinished(TransferStatus.Cancelled)
        transferJob?.cancel()
        _progress.value = null
        _status.value = TransferStatus.Idle
    }

    fun startDiscovery() {
        _devicesScanning.value = true
        viewModelScope.launch {
            delay(1200)
            _devicesScanning.value = false
            if (_devices.value.isEmpty()) {
                _devices.value = listOf(
                    Device("pc-1", "桌面机 · Windows", "192.168.1.44", Device.Via.Lan, false),
                    Device("mac-1", "MacBook Air", "192.168.1.98", Device.Via.Lan, false)
                )
            }
        }
    }

    fun pingPeer(id: String) {
        _devices.value = _devices.value.map {
            if (it.id == id) it.copy(connected = !it.connected) else it
        }
        val peer = _devices.value.find { it.id == id }
        if (peer?.connected == true) {
            _status.value = TransferStatus.Connecting
            nudgeStatusToTransferring()
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

    /** Show a peer in the device list (demo until real discovery lands). */
    private fun fakePeer(name: String, address: String?) {
        if (_devices.value.any { it.name == name }) return
        _devices.value = _devices.value + Device(
            id = name.hashCode().toString(),
            name = name,
            address = address,
            via = if (address != null) Device.Via.Lan else Device.Via.Rendezvous,
            connected = true
        )
    }

    private fun recordProgress(phase: Int, transferred: Long, total: Long, speed: Long) {
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
        if (total > 0 && transferred >= total) {
            _status.value = TransferStatus.Completed
            recordFinished(TransferStatus.Completed)
            _progress.value = null
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
        viewModelScope.launch {
            delay(1500)
            _status.value = TransferStatus.Idle
        }
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
        super.onCleared()
    }
}