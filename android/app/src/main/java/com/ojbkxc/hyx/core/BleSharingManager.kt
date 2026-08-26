package com.ojbkxc.hyx.core

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.net.ConnectivityManager
import android.os.Build
import android.os.ParcelUuid
import java.net.Inet4Address
import java.util.UUID

/**
 * HyX BLE 服务：广告本机 IP + 扫描邻居 IP。
 *
 * 与 Flutter app 的 `ble_sharing.dart` 共用同一协议：把本机局域网 IPv4 编码进一个
 * 保留的 16 字节 service UUID 尾部 8 位十六进制（4 字节 = 一个 IP）。完整 UUID =
 * `[SERVICE_PREFIX]` + `XXXXXXXX`（IP 的 4 字节十六进制）。扫描端读到邻居的
 * service UUID 后解出候选 IP，通过 [onCandidateIp] 交给上层做 Rust 单播探测。
 *
 * BLE 只负责"交换 IP"（同局域网不同网段设备发现的补充通道），**不参与在线判定**：
 * 候选 IP 能否互通、是否在线，仍由 Rust 核心的单播探测决定。所有不支持的路径
 * （无蓝牙适配器 / 无广播能力 / 缺权限）内部 try/catch 静默跳过，不影响主流程。
 */
internal class BleSharingManager(
    /** 扫描到来自其他 HyX 终端的候选 IP 时回调（可能高频，调用方需自行去重）。 */
    var onCandidateIp: ((String) -> Unit)? = null
) {
    private var adapter: BluetoothAdapter? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var scanner: BluetoothLeScanner? = null
    private var started = false
    private var advertising = false

    private val advertiseCallback = object : AdvertiseCallback() {}

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val ip = decodeIpFromScanResult(result) ?: return
            onCandidateIp?.invoke(ip)
        }
    }

    /** 优雅启动：广告本机 IP + 开始扫描邻居。重复调用为幂等（内部 [started] 置位）。 */
    @SuppressLint("MissingPermission")
    fun start(context: Context) {
        if (started) return
        val appCtx = context.applicationContext
        val bm = appCtx.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager ?: return
        val a = bm.adapter ?: return
        adapter = a

        // 连不上局域网（无 IP）则无需广播/扫描。
        val ip = localIpv4(appCtx) ?: return
        val uuid = encodeUuid(ip) ?: return

        tryAdvertise(uuid)
        tryScan()
        started = true
    }

    @SuppressLint("MissingPermission")
    private fun tryAdvertise(uuid: String) {
        try {
            val a = adapter ?: return
            if (!a.isMultipleAdvertisementSupported) return
            val adv = a.bluetoothLeAdvertiser ?: return
            advertiser = adv
            val data = AdvertiseData.Builder()
                .setIncludeDeviceName(true)
                .addServiceUuid(ParcelUuid(UUID.fromString(uuid)))
                .build()
            val settings = AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_POWER)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
                .setConnectable(true)
                .build()
            adv.startAdvertising(settings, data, advertiseCallback)
            advertising = true
        } catch (t: Throwable) {
            android.util.Log.w("BleSharing", "BLE advertise failed: ${t.message}")
        }
    }

    @SuppressLint("MissingPermission")
    private fun tryScan() {
        try {
            val a = adapter ?: return
            val s = a.bluetoothLeScanner ?: return
            scanner = s
            // 低功耗扫描（对齐 btleplug 的 low-energy 策略，节能并减少无关回调）。
            // 不做 serviceUuid 过滤：广告 UUID 携带 IP 后缀，无法按固定前缀精确匹配，
            // 前缀识别放在 onScanResult 的 decode 阶段完成。
            val settings = ScanSettings.Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_POWER)
                .build()
            s.startScan(null, settings, scanCallback)
        } catch (t: Throwable) {
            android.util.Log.w("BleSharing", "BLE scan failed: ${t.message}")
        }
    }

    /** 停止广告与扫描。 */
    @SuppressLint("MissingPermission")
    fun stop() {
        if (!started) return
        started = false
        try {
            if (advertising) advertiser?.stopAdvertising(advertiseCallback)
        } catch (t: Throwable) {
            // 忽略：停止失败无副作用。
        }
        try {
            scanner?.stopScan(scanCallback)
        } catch (t: Throwable) {
            // 忽略.
        }
        advertising = false
    }

    private fun decodeIpFromScanResult(result: ScanResult): String? {
        val record = result.scanRecord ?: return null
        val uuids = record.serviceUuids ?: emptyList<ParcelUuid>()
        for (u in uuids) {
            decodeUuid(u.uuid.toString())?.let { return it }
        }
        return null
    }

    companion object {
        /** HyX BLE service UUID 固定前缀（大写）。后 8 位十六进制（4 字节）编码 IPv4。 */
        const val SERVICE_PREFIX = "785A5958-0000-0000-0000-0000"

        /** 获取本机第一个合适的局域网 IPv4；无可用地址返回 null。 */
        fun localIpv4(context: Context): String? {
            modernLocalIp(context)?.let { return it }
            return legacyWifiIp(context)
        }

        @SuppressLint("MissingPermission")
        private fun modernLocalIp(context: Context): String? {
            return try {
                val cm = context.applicationContext
                    .getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return null
                if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return null
                val network = cm.activeNetwork ?: return null
                val lp = cm.getLinkProperties(network) ?: return null
                for (addr in lp.linkAddresses) {
                    val ip = addr.address ?: continue
                    if (ip !is Inet4Address) continue
                    if (ip.isLoopbackAddress) continue
                    val host = ip.hostAddress ?: continue
                    if (host.startsWith("169.254.")) continue
                    return host
                }
                null
            } catch (t: Throwable) {
                null
            }
        }

        @SuppressLint("MissingPermission")
        private fun legacyWifiIp(context: Context): String? {
            return try {
                val wm = context.applicationContext
                    .getSystemService(Context.WIFI_SERVICE) as? android.net.wifi.WifiManager ?: return null
                val ip = wm.connectionInfo.ipAddress ?: return null
                if (ip == 0) return null
                String.format(
                    "%d.%d.%d.%d",
                    ip and 0xff,
                    (ip shr 8) and 0xff,
                    (ip shr 16) and 0xff,
                    (ip shr 24) and 0xff
                )
            } catch (t: Throwable) {
                null
            }
        }

        /** IPv4 → 完整 HyX service UUID（尾部 8 位十六进制）。非法地址返回 null。 */
        fun encodeUuid(ip: String): String? {
            val hex = ipToHex(ip) ?: return null
            return SERVICE_PREFIX + hex
        }

        /** 从完整 UUID 尾部 8 位十六进制还原 IPv4；非 HyX 前缀或格式非法返回 null。 */
        fun decodeUuid(uuid: String): String? {
            val upper = uuid.trim().uppercase()
            if (!upper.startsWith(SERVICE_PREFIX)) return null
            val hex = upper.substring(SERVICE_PREFIX.length)
            if (hex.length != 8) return null
            val bytes = IntArray(4)
            for (i in 0 until 4) {
                bytes[i] = hex.substring(i * 2, i * 2 + 2).toIntOrNull(16) ?: return null
            }
            return "${bytes[0]}.${bytes[1]}.${bytes[2]}.${bytes[3]}"
        }

        private fun ipToHex(ip: String): String? {
            val parts = ip.split('.')
            if (parts.size != 4) return null
            val sb = StringBuilder()
            for (p in parts) {
                val v = p.toIntOrNull() ?: return null
                if (v < 0 || v > 255) return null
                sb.append(String.format("%02x", v))
            }
            return sb.toString()
        }
    }
}