package com.ojbkxc.hyx.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.background
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import android.content.Context
import android.net.Uri
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeviceHub
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Send
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.documentfile.provider.DocumentFile
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.ui.components.StatusBadge
import com.ojbkxc.hyx.ui.model.Device
import com.ojbkxc.hyx.ui.theme.HyxGreen
import java.io.File

@Composable
fun DevicesScreen(controller: HyXCoreController) {
    val devices by controller.devices.collectAsState()
    val scanning by controller.devicesScanning.collectAsState()

    val online = devices.filter { it.online }
    val history = devices.filter { !it.online }

    // LAN 直连发送：选目标设备 → 系统文件选择器 → copyToCache → startLanSend。
    // sendTarget 记住用户点了哪个设备卡片，文件选择回调里取回。
    val context = LocalContext.current
    var sendTarget by remember { mutableStateOf<Device?>(null) }
    val pickFile = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri ->
        val target = sendTarget
        sendTarget = null
        if (uri != null && target != null) {
            val addr = target.address
            if (addr != null) {
                copyToCache(context, uri)?.let {
                    controller.startLanSend(addr, it, target.fingerprint)
                }
            }
        }
    }

    Column(Modifier.fillMaxSize().padding(horizontal = 20.dp, vertical = 12.dp)) {
        Text(
            stringResource(R.string.tab_devices_title),
            style = MaterialTheme.typography.headlineSmall,
            color = MaterialTheme.colorScheme.onBackground
        )
        Spacer(Modifier.height(4.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (scanning) {
                CircularProgressIndicator(modifier = Modifier.size(14.dp), strokeWidth = 2.dp)
                Spacer(Modifier.size(8.dp))
            }
            Text(
                if (scanning) stringResource(R.string.scanning_lan)
                else stringResource(R.string.devices_count, devices.size),
                fontSize = 13.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        Spacer(Modifier.height(8.dp))

        LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            item(key = "online_header") { SectionHeader(stringResource(R.string.online_devices), online.size) }
            if (online.isEmpty()) {
                item(key = "online_empty") {
                    EmptyRow(stringResource(R.string.no_online_devices))
                }
            } else {
                items(online, key = { it.id }) { device ->
                    OnlineDeviceCard(
                        device,
                        onToggle = { controller.toggleAllowTransfer(device.id) },
                        onSend = {
                            sendTarget = device
                            pickFile.launch(arrayOf("*/*"))
                        }
                    )
                }
            }

            item(key = "history_header") { SectionHeader(stringResource(R.string.history_devices), history.size) }
            if (history.isEmpty()) {
                item(key = "history_empty") {
                    EmptyRow(stringResource(R.string.no_history_devices))
                }
            } else {
                items(history, key = { it.id }) { device ->
                    HistoryDeviceCard(
                        device,
                        onToggle = { controller.toggleAllowTransfer(device.id) },
                        onDelete = { controller.removeHistoryDevice(device.id) }
                    )
                }
            }
        }
    }
}

@Composable
private fun SectionHeader(title: String, count: Int) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(title, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, color = MaterialTheme.colorScheme.onBackground)
        Spacer(Modifier.size(8.dp))
        Text(
            stringResource(R.string.device_count, count),
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}

@Composable
private fun EmptyRow(text: String) {
    Text(
        text,
        modifier = Modifier.fillMaxWidth().padding(vertical = 14.dp),
        fontSize = 13.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant
    )
}

@Composable
private fun OnlineDeviceCard(device: Device, onToggle: () -> Unit, onSend: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Avatar(device, online = true)
            Spacer(Modifier.size(12.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(device.name, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.weight(1f))
                    StatusBadge(text = stringResource(deviceViaLabelRes(device.via)), active = true)
                }
                Spacer(Modifier.height(2.dp))
                Text(
                    device.address ?: "",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Spacer(Modifier.size(12.dp))
            AllowToggle(device.allowTransfer, onToggle)
            // 发送给此设备：仅在线且有 address 时可用（LAN 直连需要明确地址）。
            // 配对码设备（address 为 null）不显示此按钮，仍走 TransferScreen 的配对路径。
            if (device.address != null) {
                IconButton(onClick = onSend) {
                    Icon(
                        Icons.Outlined.Send,
                        contentDescription = stringResource(R.string.send_to_device),
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(20.dp)
                    )
                }
            }
        }
    }
}

@Composable
private fun HistoryDeviceCard(device: Device, onToggle: () -> Unit, onDelete: () -> Unit) {
    val dim = MaterialTheme.colorScheme.onSurfaceVariant
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(Modifier.padding(start = 14.dp, top = 6.dp, bottom = 6.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Avatar(device, online = false)
            Spacer(Modifier.size(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    device.name,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    color = dim,
                    modifier = Modifier.weight(1f)
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    device.address ?: "",
                    fontSize = 12.sp,
                    color = dim
                )
            }
            Spacer(Modifier.size(8.dp))
            AllowToggle(device.allowTransfer, onToggle)
            IconButton(onClick = onDelete) {
                Icon(
                    Icons.Outlined.Delete,
                    contentDescription = stringResource(R.string.delete_device),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(18.dp)
                )
            }
        }
    }
}

@Composable
private fun Avatar(device: Device, online: Boolean) {
    val tint = if (online) HyxGreen else MaterialTheme.colorScheme.onSurfaceVariant
    Box(
        modifier = Modifier
            .size(44.dp)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center
    ) {
        Icon(
            Icons.Outlined.DeviceHub,
            contentDescription = null,
            tint = tint,
            modifier = Modifier.size(22.dp)
        )
    }
}

/** Pill button toggling 接收 / 禁止 — only two states, taps switch. */
@Composable
private fun AllowToggle(allow: Boolean, onToggle: () -> Unit) {
    val bg: Color
    val fg: Color
    val border: Color
    val label: String
    if (allow) {
        label = stringResource(R.string.allow_transfer)
        bg = HyxGreen
        fg = Color.White
        border = Color.Transparent
    } else {
        label = stringResource(R.string.block_transfer)
        bg = MaterialTheme.colorScheme.errorContainer
        fg = MaterialTheme.colorScheme.onErrorContainer
        border = MaterialTheme.colorScheme.error
    }
    Surface(
        onClick = onToggle,
        shape = CircleShape,
        color = bg,
        border = BorderStroke(1.dp, border),
        modifier = Modifier.height(32.dp)
    ) {
        Box(contentAlignment = Alignment.Center, modifier = Modifier.padding(horizontal = 16.dp)) {
            Text(label, color = fg, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        }
    }
}

private fun deviceViaLabelRes(via: Device.Via): Int = when (via) {
    Device.Via.Lan -> R.string.via_lan
    Device.Via.Rendezvous -> R.string.via_rendezvous
}

/**
 * 将 content URI 复制到 app cache，使 Rust 内核能按路径读取。
 * 与 TransferScreen.kt 的 copyToCache 实现一致（未提取到公共包以避免改 TransferScreen）。
 */
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