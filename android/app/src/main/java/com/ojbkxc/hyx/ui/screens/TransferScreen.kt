package com.ojbkxc.hyx.ui.screens

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import android.content.Context
import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Article

import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton

import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
import androidx.documentfile.provider.DocumentFile
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.core.LogCollector
import com.ojbkxc.hyx.ui.components.MetricCardRow

import com.ojbkxc.hyx.ui.components.RingProgress
import com.ojbkxc.hyx.ui.components.StatusBadge
import com.ojbkxc.hyx.ui.components.formatBytes
import com.ojbkxc.hyx.ui.components.formatDuration
import com.ojbkxc.hyx.ui.components.formatSpeed

import com.ojbkxc.hyx.ui.model.TransferStatus
import java.io.File

@Composable
fun TransferScreen(
    controller: HyXCoreController
) {
    val status by controller.status.collectAsState()
    val progress by controller.progress.collectAsState()
    val settings by controller.settings.collectAsState()
    val autoListening by controller.autoListening.collectAsState()

    var showLogSheet by remember { mutableStateOf(false) }

    val context = LocalContext.current
    val pickFile = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri ->
        // 选文件后仅缓存到 app cache，实际发送需在设备列表选择目标设备后发起
        // （对齐 Flutter app：发送从设备列表发起，不再走配对码路径）。
        if (uri != null) {
            copyToCache(context, uri)
        }
    }

    val busy = status == TransferStatus.Transferring || status == TransferStatus.Connecting

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        // 标题行 + 日志按钮
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                stringResource(R.string.tab_transfer_title),
                style = MaterialTheme.typography.headlineSmall,
                color = MaterialTheme.colorScheme.onBackground
            )
            Spacer(Modifier.weight(1f))
            IconButton(onClick = { showLogSheet = true }) {
                Icon(
                    Icons.Outlined.Article,
                    contentDescription = stringResource(R.string.log_title),
                    tint = MaterialTheme.colorScheme.onBackground
                )
            }
        }

        Spacer(Modifier.height(16.dp))


        if (busy && progress != null) {
            TransferringPanel(progress = progress!!, onCancel = controller::cancelTransfer)
        } else {
            TransferIdlePanel(
                autoListening = autoListening,
                onSendFiles = { pickFile.launch(arrayOf("*/*")) }
            )
        }

        Spacer(Modifier.height(20.dp))

        EngineSettingsCard(settings = settings, onChange = { updated -> controller.updateSettings { updated } })

        Spacer(Modifier.height(8.dp))
        StatusBadge(text = stringResource(statusTextRes(status)), active = busy)
        Spacer(Modifier.height(4.dp))
    }

    if (showLogSheet) {
        LogSheet(
            logs = LogCollector.logs.collectAsState().value,
            onClear = LogCollector::clear,
            onDismiss = { showLogSheet = false }
        )
    }
}


@Composable
private fun TransferringPanel(
    progress: com.ojbkxc.hyx.ui.model.TransferProgress,
    onCancel: () -> Unit
) {
    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
        Text(
            progress.name,
            style = MaterialTheme.typography.titleMedium,
            maxLines = 1
        )
        Spacer(Modifier.height(8.dp))
        RingProgress(progress.fraction)
        Spacer(Modifier.height(12.dp))
        Text(
            "${(progress.fraction * 100).toInt()}%",
            fontSize = 22.sp, fontWeight = FontWeight.Bold
        )
        Spacer(Modifier.height(20.dp))
        MetricCardRow(
            listOf(
                stringResource(R.string.total_size) to formatBytes(progress.totalBytes),
                stringResource(R.string.transferred) to formatBytes(progress.transferredBytes),
                stringResource(R.string.speed) to formatSpeed(progress.speedBps),
                stringResource(R.string.remaining) to formatDuration(etaMs(progress))
            )
        )
        Spacer(Modifier.height(20.dp))
        OutlinedButton(onClick = onCancel) { Text(stringResource(R.string.cancel)) }
    }
}

@Composable
private fun TransferIdlePanel(
    autoListening: Boolean,
    onSendFiles: () -> Unit
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        // 主操作：选文件发送（实际发送在设备列表选择目标设备后发起）
        Button(
            onClick = onSendFiles,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(16.dp)
        ) { Text(stringResource(R.string.pair_share)) }

        // 后台自动监听提示：app 启动即监听 LAN 接收，无需用户手动操作
        if (autoListening) {
            Spacer(Modifier.height(4.dp))
            Row(
                modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center
            ) {
                Box(
                    modifier = Modifier
                        .size(8.dp)
                        .background(
                            color = MaterialTheme.colorScheme.tertiary,
                            shape = RoundedCornerShape(50)
                        )
                )
                Spacer(Modifier.size(6.dp))
                Text(
                    stringResource(R.string.auto_listening_hint),
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }
    }
}

@Composable
private fun EngineSettingsCard(
    settings: com.ojbkxc.hyx.ui.model.EngineSettings,
    onChange: (com.ojbkxc.hyx.ui.model.EngineSettings) -> Unit
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(16.dp),
        modifier = Modifier.fillMaxWidth()
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(stringResource(R.string.engine), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(12.dp))
            SettingSwitch(stringResource(R.string.engine_compression), settings.compression) {
                onChange(settings.copy(compression = it))
            }
            SettingSwitch(stringResource(R.string.engine_aggregation), settings.aggregation) {
                onChange(settings.copy(aggregation = it))
            }
        }
    }
}

@Composable
private fun SettingSwitch(label: String, checked: Boolean, onToggle: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, modifier = Modifier.weight(1f), fontSize = 14.sp)
        Switch(checked = checked, onCheckedChange = onToggle)
    }
}

private fun statusTextRes(status: TransferStatus): Int = when (status) {
    TransferStatus.Idle -> R.string.status_idle
    TransferStatus.Pairing -> R.string.status_pairing
    TransferStatus.Connecting -> R.string.status_connecting
    TransferStatus.Transferring -> R.string.status_transferring
    TransferStatus.Completed -> R.string.status_completed
    TransferStatus.Failed -> R.string.status_failed
    TransferStatus.Cancelled -> R.string.status_cancelled
}

/** Copy a content URI into the app cache so the Rust kernel can read it by path. */
private fun copyToCache(context: Context, uri: Uri): String? = try {
    val name = DocumentFile.fromSingleUri(context, uri)?.name ?: "share.bin"
    val destDir = File(context.cacheDir, "hyx_send").apply { mkdirs() }
    val dest = File(destDir, name)
    val input = context.contentResolver.openInputStream(uri) ?: return null
    input.use { i -> dest.outputStream().use { o -> i.copyTo(o) } }
    dest.absolutePath
} catch (e: Exception) {
    null
}

private fun etaMs(p: com.ojbkxc.hyx.ui.model.TransferProgress): Long {
    if (p.speedBps <= 0) return 0
    val remaining = (p.totalBytes - p.transferredBytes).coerceAtLeast(0)
    return (remaining / p.speedBps * 1000).toLong()
}