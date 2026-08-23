package com.ojbkxc.hyx

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
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


    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
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
