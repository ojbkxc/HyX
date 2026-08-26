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

    // Bug1: Wi-Fi Direct 的 discoverPeers 是一次性的，发现结果约 30s 后过期。
    // 用定时 Runnable 周期性重新发现，保持对端可持续被发现。
    private val discoverIntervalMs = 30_000L
    private val discoverRunnable = object : Runnable {
        override fun run() {
            if (!started) return
            discoverPeers()
            mainHandler.postDelayed(this, discoverIntervalMs)
        }
    }

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
                // Bug1: 启动周期性重新发现，避免 30s 后发现结果过期。
                mainHandler.postDelayed(discoverRunnable, discoverIntervalMs)
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
        // Bug1: 取消周期性发现任务。
        mainHandler.removeCallbacks(discoverRunnable)
        // Bug3: 清理可能残留的 P2P 组，避免下次启动时残留组干扰发现。
        // 没有活跃组时 removeGroup 会失败，try/catch 静默忽略。
        try {
            val m = manager
            val c = channel
            if (m != null && c != null) {
                m.removeGroup(c, SimpleListener("removeGroup"))
            }
        } catch (t: Throwable) {
            android.util.Log.w("WifiDirect", "removeGroup failed: ${t.message}")
        }
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
                // Bug2: 遍历所有 AVAILABLE 状态的 peer，对每个都尝试建组。
                // ensureRole 已有 actedMacs 去重和 acting 守卫，不会重复建组。
                deviceList
                    .filter {
                        it.status == WifiP2pDevice.AVAILABLE &&
                            !it.deviceAddress.isNullOrBlank()
                    }
                    .forEach { ensureRole(it.deviceAddress) }
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
        // Bug5: ourMac 为 null 时（某些设备不发送 THIS_DEVICE_CHANGED）不直接 return，
        // 否则 actedMacs 已 add 但永远无法判定，导致永远无法建组。
        // 改为：ourMac 为 null 时直接 connect，让对端（可能有 ourMac）来决定角色。
        val own = ourMac
        mainHandler.post {
            try {
                val m = manager ?: return@post
                val c = channel ?: return@post
                acting = true
                if (own == null) {
                    // Bug5: 本机 MAC 未知，主动 connect 入组，由对端决定角色。
                    m.connect(
                        c,
                        WifiP2pConfig().apply {
                            deviceAddress = peerMac
                            wps.setup = android.net.wifi.WpsInfo.PBC
                        },
                        RoleActionListener(peerMac, "connect")
                    )
                } else if (own.compareTo(peerMac) < 0) {
                    // 本机当 Group Owner（软热点），等待对端 connect 入组。
                    m.createGroup(c, RoleActionListener(peerMac, "createGroup"))
                } else {
                    // 主动连接对端（入组）。
                    m.connect(
                        c,
                        WifiP2pConfig().apply {
                            deviceAddress = peerMac
                            wps.setup = android.net.wifi.WpsInfo.PBC
                        },
                        RoleActionListener(peerMac, "connect")
                    )
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
        if (info?.groupFormed != true) {
            // Bug6: 连接断开（info 为 null 或 groupFormed=false）时重置状态，
            // 否则 acting/isGroupOwner 残留会导致无法重新建组。
            isGroupOwner = false
            acting = false
            // 清理已处理 MAC 集合，允许重新对已断开的 peer 建组。
            actedMacs.clear()
            // 重新启动发现，开始新一轮建组尝试。
            if (started) discoverPeers()
            return
        }
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

    /**
     * Bug4: 角色判定专用 Listener。createGroup/connect 失败时必须重置 acting 并移除
     * 对应 peerMac，否则 acting 残留会导致后续永远无法再建组。
     * SimpleListener 是通用的，不知道是哪个 peer，故此处单独建一个。
     */
    private inner class RoleActionListener(
        private val peerMac: String,
        private val tag: String
    ) : WifiP2pManager.ActionListener {
        override fun onSuccess() = Unit
        override fun onFailure(reason: Int) {
            acting = false
            actedMacs.remove(peerMac)
            android.util.Log.w(
                "WifiDirect",
                "$tag failed: reason=$reason, peerMac=$peerMac"
            )
        }
    }
}