package com.ojbkxc.hyx

import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.Manifest
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier

import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.core.UpdateChecker
import com.ojbkxc.hyx.core.UpdateInfo
import com.ojbkxc.hyx.ui.HyXNavigation
import com.ojbkxc.hyx.ui.theme.HyXTheme

class MainActivity : ComponentActivity() {

    // Single activity-scoped ViewModel shared by all three tabs.
    private val controller: HyXCoreController by viewModels()

    // 蓝牙/Wi-Fi 直连权限授权回调：授权后重新尝试启动（此前因缺权限静默失败）。
    private val blePermLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) {
        controller.startBleDiscovery()
        if (controller.wifiDirectEnabled.value) controller.setWifiDirectEnabled(true)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        requestBlePermissions()
        setContent {
            HyXTheme {
                // 更新检测：发现新版本时弹窗提示下载。
                // fire-and-forget：LaunchedEffect 只在首次进入时执行一次，
                // 检测失败或无新版本时静默无操作，不影响主 UI。
                var updateInfo by remember { mutableStateOf<UpdateInfo?>(null) }
                LaunchedEffect(Unit) {
                    updateInfo = UpdateChecker.check(BuildConfig.VERSION_NAME)
                }
                updateInfo?.let { info ->
                    UpdateDialog(
                        info = info,
                        onDownload = {
                            openInBrowser(info.downloadUrl)
                            updateInfo = null
                        },
                        onDismiss = { updateInfo = null }
                    )
                }

                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    HyXNavigation(controller = controller)
                }
            }
        }
        // 应用可能由系统分享面板启动，解析分享 Intent 并交给 controller 处理。
        handleShareIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleShareIntent(intent)
    }

    /**
     * 请求直连发现所需权限：Android 12+ 需要 BLUETOOTH_SCAN / BLUETOOTH_ADVERTISE，
     * Android 13+ 的 Wi-Fi Direct 发现还需 NEARBY_WIFI_DEVICES；更早版本依赖定位权限
     * （已声明于 manifest）。授权后 [blePermLauncher] 会重新触发蓝牙广播/扫描，并
     * 在用户已开启 Wi-Fi 直连开关时重试启动（此前因缺权限静默失败）。
     */
    private fun requestBlePermissions() {
        val perms = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            buildList {
                add(Manifest.permission.BLUETOOTH_SCAN)
                add(Manifest.permission.BLUETOOTH_ADVERTISE)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    add(Manifest.permission.NEARBY_WIFI_DEVICES)
                }
            }.toTypedArray()
        } else {
            arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }
        val missing = perms.filter {
            checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isNotEmpty()) {
            blePermLauncher.launch(missing.toTypedArray())
        } else {
            controller.startBleDiscovery()
            if (controller.wifiDirectEnabled.value) controller.setWifiDirectEnabled(true)
        }
    }

    /**
     * 解析系统分享 Intent（ACTION_SEND / ACTION_SEND_MULTIPLE），提取文件 URI 列表
     * 并交给 [HyXCoreController.handleSharedUris] 复制到缓存后发送给首个在线设备。
     */
    @Suppress("DEPRECATION")
    private fun handleShareIntent(intent: Intent) {
        if (intent.action != Intent.ACTION_SEND && intent.action != Intent.ACTION_SEND_MULTIPLE) return
        val uris = when (intent.action) {
            Intent.ACTION_SEND -> {
                val uri = intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM) ?: return
                listOf(uri)
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM) ?: return
            }
            else -> return
        }
        if (uris.isEmpty()) return
        controller.handleSharedUris(uris)
    }

    /** 用系统浏览器打开下载链接。 */
    private fun openInBrowser(url: String) {
        try {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            startActivity(intent)
        } catch (_: Exception) {
            // 没有可处理 http(s) 的浏览器，静默忽略
        }
    }
}

/**
 * 发现新版本时的提示弹窗（对齐 Flutter app/lib/pages/home_page.dart 的 _checkForUpdate 弹窗）。
 */
@Composable
private fun UpdateDialog(
    info: UpdateInfo,
    onDownload: () -> Unit,
    onDismiss: () -> Unit
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("发现新版本 ${info.version}") },
        text = {
            Text(info.releaseNotes ?: "立即更新以获取最新功能与修复。")
        },
        confirmButton = {
            TextButton(onClick = onDownload) { Text("下载") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("稍后") }
        }
    )
}
