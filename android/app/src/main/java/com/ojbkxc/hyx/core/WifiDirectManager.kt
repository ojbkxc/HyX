package com.ojbkxc.hyx.core

import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.wifi.p2p.WifiP2pConfig
import android.net.wifi.p2p.WifiP2pDevice
import android.net.wifi.p2p.WifiP2pInfo
import android.net.wifi.p2p.WifiP2pManager
import android.os.Handler
import android.os.Looper
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/**
 * HyX 的 Wi-Fi Direct 直连通道（对齐小米互传：无需连热点也能点对点互传）。
 *
 * BLE 只能"交换 IP"且要求双方已处于同一可路由网络；而两台手机在**都不连接任何
 * 热点**时，Wi-Fi Direct 可以让系统在两机间建立一条点对点链路（一台当
 * Group Owner / 软热点，另一台作为 Client 入组），并在 P2P 网段（默认为
 * `192.168.49.x`）给双方分配可路由 IP。
 *
 * 本类只负责：发现附近 HyX 终端 → 自动确定角色（MAC 小者当 GO）→ 建立 P2P 组 →
 * 解析对端 P2P IP，并通过 [onPeerIp] 交给上层。上层复用与 BLE 完全相同的
 * `hyxProbeIp` 单播探测路径：对端在线与否、能否传文件仍由 Rust 核心决定。
 * 所有不支持/缺权限/失败的路径内部 try/catch 静默跳过，不影响主流程。
 */
@SuppressLint("MissingPermission")
internal class WifiDirectManager(
    /** 解析到来自对端终端的 P2P IP 时回调（可能多次，调用方需自行去重）。 */
    var onPeerIp: ((String) -> Unit)? = null
) {
    private var manager: WifiP2pManager? = null
    private var channel: WifiP2pManager.Channel? = null
    private var contextRef: Context? = null
    private var registered = false
    private var started = false
    private var acting = false
    private var isGroupOwner = false

    // 本机在 THIS_DEVICE_CHANGED 广播里的 deviceAddress（MAC），用于角色判定。
    private var ourMac: String? = null
    // 已见过的对端 MAC，避免反复触发角色判定/建组。
    private val actedMacs = ConcurrentHashMap.newKeySet<String>()

    private val mainHandler = Handler(Looper.getMainLooper())

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            try {
                when (intent.action) {
                    WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION -> {
                        val state = intent.getIntExtra(WifiP2pManager.EXTRA_WIFI_STATE, -1)
                        if (state == WifiP2pManager.WIFI_P2P_STATE_ENABLED) discoverPeers()
                    }
                    WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION -> requestPeers()
                    WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION -> {
                        @Suppress("DEPRECATION")
                        val info = intent.getParcelableExtra<WifiP2pInfo>(
                            WifiP2pManager.EXTRA_WIFI_P2P_INFO
                        )
                        handleConnection(info)
                    }
                    WifiP2pManager.WIFI_P2P_THIS_DEVICE_CHANGED_ACTION -> {
                        @Suppress("DEPRECATION")
                        val dev = intent.getParcelableExtra<WifiP2pDevice>(
                            WifiP2pManager.EXTRA_WIFI_P2P_DEVICE
                        )
                        ourMac = dev?.deviceAddress
                    }
                }
            } catch (t: Throwable) {
                android.util.Log.w("WifiDirect", "onReceive failed: ${t.message}")
            }
        }
    }

    /** 优雅启动：注册 P2P 广播 + 开始发现。重复调用幂等。需主线程调用。 */
    fun start(context: Context) {
        if (started) return
        started = true
        val appCtx = context.applicationContext
        contextRef = appCtx
        // requestPeers / createGroup / connect 等 Api 必须在主 Looper 上使用。
        mainHandler.post {
            try {
                val m = appCtx.getSystemService(Context.WIFI_P2P_SERVICE) as? WifiP2pManager
                    ?: return@post
                manager = m
                channel = m.initialize(appCtx, Looper.getMainLooper(), null) ?: return@post
                val filter = IntentFilter().apply {
                    addAction(WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION)
                    addAction(WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION)
                    addAction(WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION)
                    addAction(WifiP2pManager.WIFI_P2P_THIS_DEVICE_CHANGED_ACTION)
                }
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
                    appCtx.registerReceiver(receiver, filter, Context.RECEIVER_EXPORTED)
                } else {
                    appCtx.registerReceiver(receiver, filter)
                }
                registered = true
                discoverPeers()
            } catch (t: Throwable) {
                android.util.Log.w("WifiDirect", "start failed: ${t.message}")
            }
        }
    }

    /** 停止：退订广播并复位状态。 */
    fun stop() {
        if (!started && !registered) return
        started = false
        isGroupOwner = false
        acting = false
        actedMacs.clear()
        contextRef?.let { ctx ->
            try {
                if (registered) ctx.unregisterReceiver(receiver)
            } catch (t: Throwable) {
                // ignore: 未注册时 unregister 会抛异常。
            }
        }
        registered = false
        contextRef = null
    }

    private fun discoverPeers() {
        val m = manager ?: return
        val c = channel ?: return
        try {
            m.discoverPeers(c, SimpleListener("discoverPeers"))
        } catch (t: Throwable) {
            android.util.Log.w("WifiDirect", "discoverPeers failed: ${t.message}")
        }
    }

    private fun requestPeers() {
        val m = manager ?: return
        val c = channel ?: return
        try {
            m.requestPeers(c) { peers ->
                val deviceList = peers.deviceList
                if (deviceList.isEmpty()) return@requestPeers
                val peer = deviceList.firstOrNull { it.status == WifiP2pDevice.AVAILABLE }
                    ?: deviceList.first()
                if (peer.deviceAddress.isNullOrBlank()) return@requestPeers
                ensureRole(peer.deviceAddress)
            }
        } catch (t: Throwable) {
            android.util.Log.w("WifiDirect", "requestPeers failed: ${t.message}")
        }
    }

    /**
     * 角色判定：同一 P2P 组的建组必须恰好一台当 GO。用本机与对端 MAC 字典序比较，
     * 较小者 `createGroup()`（当软热点），较大者 `connect()`（加入对方组）。
     * 已有动作后不再重复，避免两边同时建组冲突。
     */
    private fun ensureRole(peerMac: String) {
        if (acting) return
        if (!actedMacs.add(peerMac)) return
        val own = ourMac ?: return // THIS_DEVICE 尚未到来，等下次事件再判定
        mainHandler.post {
            try {
                val m = manager ?: return@post
                val c = channel ?: return@post
                acting = true
                if (own.compareTo(peerMac) < 0) {
                    // 本机当 Group Owner（软热点），等待对端 connect 入组。
                    m.createGroup(c, SimpleListener("createGroup"))
                } else {
                    // 主动连接对端（入组）。
                    m.connect(c, WifiP2pConfig().apply {
                        deviceAddress = peerMac
                        wps.setup = android.net.wifi.WpsInfo.PBC
                    }, SimpleListener("connect"))
                }
            } catch (t: Throwable) {
                acting = false
                actedMacs.remove(peerMac)
                android.util.Log.w("WifiDirect", "ensureRole failed: ${t.message}")
            }
        }
    }

    /** P2P 组建立成功（CONNECTION_CHANGED 且 groupFormed）后解析对端 IP。 */
    private fun handleConnection(info: WifiP2pInfo?) {
        if (info?.groupFormed != true) return
        isGroupOwner = info.isGroupOwner
        val peerIp: String? = if (isGroupOwner) {
            resolveClientIpViaArp()
        } else {
            info.groupOwnerAddress?.hostAddress
        }
        if (!peerIp.isNullOrBlank()) {
            onPeerIp?.invoke(peerIp)
        } else if (isGroupOwner) {
            // ARP 尚未就绪：稍后重试几次，客户端刚拿到的 DHCP 租约通常很快可解析。
            retryArp(0)
        }
    }

    /** GO 侧：从系统 ARP 表读取 p2p 接口上对端(MAC 0x2 完成)的 IPv4。 */
    private fun resolveClientIpViaArp(): String? {
        return try {
            val lines = File("/proc/net/arp").readLines()
            if (lines.size < 2) return null
            for (i in 1 until lines.size) {
                val cols = lines[i].trim().split(Regex("\\s+"))
                if (cols.size >= 6 && cols[4] == "0x2" && cols[5].startsWith("p2p")) {
                    return cols[0].takeIf { it.isNotBlank() }
                }
            }
            null
        } catch (t: Throwable) {
            null
        }
    }

    private fun retryArp(attempt: Int) {
        if (!isGroupOwner || !started) return
        if (attempt >= 5) return
        mainHandler.postDelayed({
            val ip = resolveClientIpViaArp()
            if (!ip.isNullOrBlank()) {
                onPeerIp?.invoke(ip)
            } else {
                retryArp(attempt + 1)
            }
        }, 1000L)
    }

    private inner class SimpleListener(private val tag: String) : WifiP2pManager.ActionListener {
        override fun onSuccess() = Unit
        override fun onFailure(reason: Int) {
            android.util.Log.w("WifiDirect", "$tag failed: reason=$reason")
        }
    }
}