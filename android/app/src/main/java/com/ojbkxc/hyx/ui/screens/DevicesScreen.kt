package com.ojbkxc.hyx.ui.screens

import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Article
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.DevicesOther
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Radar
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.ui.text.input.KeyboardOptions
import androidx.documentfile.provider.DocumentFile
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.core.LogCollector
import com.ojbkxc.hyx.ui.model.Device
import com.ojbkxc.hyx.ui.theme.HyxGreen
import java.io.File

/**
 * 设备页 —— 对齐 Flutter home_page.dart + device_card.dart。
 *
 * 顶部：本设备名称（可点击改名）+ 扫描指示 + 日志按钮。
 * 主体：在线设备区 + 历史设备区，卡片为 Column 布局（头像/名称/地址/状态徽章 + 接收开关）。
 * 在线设备点击整卡触发文件选择 → 发送；历史设备置灰，左/右滑删除。
 *
 * LAN 发现由 [HyXCoreController.startAutoDiscovery] 每 5s 自动刷新，
 * 无手动刷新按钮（对齐 Flutter StartDiscoveryAction 的 5s 定时）。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DevicesScreen(controller: HyXCoreController) {
    val devices by controller.devices.collectAsState()
    val scanning by controller.devicesScanning.collectAsState()
    val autoListening by controller.autoListening.collectAsState()
    val wifiDirectEnabled by controller.wifiDirectEnabled.collectAsState()
    val deviceName by controller.deviceName.collectAsState()
    var showLogSheet by remember { mutableStateOf(false) }
    var showNameDialog by remember { mutableStateOf(false) }
    var pendingDelete by remember { mutableStateOf<Device?>(null) }

    val online = devices.filter { it.online }
    val history = devices.filter { !it.online }

    // LAN 直连发送：选目标设备 → 系统文件选择器 → copyToCache → startLanSend。
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

    Column(Modifier.fillMaxSize()) {
        // AppBar 区域：本设备名称（可点击改名）+ 扫描指示 + 日志按钮。
        // 去掉了刷新按钮（改为 controller.startAutoDiscovery 每 5s 自动刷新）
        // 和设置按钮（改名直接点标题）。
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            // 标题：本设备名称，点击弹出 inline 编辑对话框。
            Row(
                modifier = Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .clickable { showNameDialog = true }
                    .padding(horizontal = 4.dp, vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    deviceName.ifEmpty { stringResource(R.string.app_name) },
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onBackground,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
                Spacer(Modifier.size(6.dp))
                Icon(
                    Icons.Outlined.Edit,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
                )
            }
            Spacer(Modifier.size(8.dp))
            if (scanning) {
                CircularProgressIndicator(
                    modifier = Modifier.size(14.dp),
                    strokeWidth = 2.dp
                )
            }
            Spacer(Modifier.weight(1f))
            IconButton(onClick = { showLogSheet = true }) {
                Icon(
                    Icons.Outlined.Article,
                    contentDescription = stringResource(R.string.log_title),
                    tint = MaterialTheme.colorScheme.onBackground
                )
            }
        }

        // Wi-Fi 直连开关：无需连热点，两台设备都开启即可自动建 P2P 组互相发现。
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                "Wi-Fi 直连（无热点互传）",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Spacer(Modifier.weight(1f))
            Switch(
                checked = wifiDirectEnabled,
                onCheckedChange = { controller.setWifiDirectEnabled(it) }
            )
        }

        // 自动监听指示。
        if (autoListening) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center
            ) {
                Box(
                    modifier = Modifier
                        .size(8.dp)
                        .background(MaterialTheme.colorScheme.tertiary, CircleShape)
                )
                Spacer(Modifier.size(6.dp))
                Text(
                    stringResource(R.string.auto_listening_hint),
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }

        // 主体：空状态 or 设备列表。
        if (online.isEmpty() && history.isEmpty()) {
            EmptyState(scanning = scanning)
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 96.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp)
            ) {
                item(key = "online_header") {
                    SectionHeader(stringResource(R.string.online_devices), online.size)
                }
                if (online.isEmpty()) {
                    item(key = "online_empty") {
                        EmptySectionHint(stringResource(R.string.no_online_devices))
                    }
                } else {
                    items(online, key = { it.id }) { device ->
                        DeviceCard(
                            device = device,
                            online = true,
                            onToggle = { controller.toggleAllowTransfer(device.id) },
                            onClick = {
                                sendTarget = device
                                pickFile.launch(arrayOf("*/*"))
                            }
                        )
                    }
                }

                if (history.isNotEmpty()) {
                    item(key = "history_header") {
                        Spacer(Modifier.height(8.dp))
                        SectionHeader(stringResource(R.string.history_devices), history.size)
                    }
                    items(history, key = { it.id }) { device ->
                        val dismissState = rememberSwipeToDismissBoxState(
                            confirmValueChange = { value ->
                                if (value == SwipeToDismissBoxValue.StartToEnd ||
                                    value == SwipeToDismissBoxValue.EndToStart
                                ) {
                                    pendingDelete = device
                                    false // 回弹，等对话框确认
                                } else false
                            }
                        )
                        SwipeToDismissBox(
                            state = dismissState,
                            enableDismissFromStartToEnd = true,
                            enableDismissFromEndToStart = true,
                            backgroundContent = {
                                Box(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .clip(RoundedCornerShape(16.dp))
                                        .background(MaterialTheme.colorScheme.error)
                                        .padding(horizontal = 20.dp),
                                    contentAlignment = Alignment.CenterEnd
                                ) {
                                    Icon(
                                        Icons.Outlined.Delete,
                                        contentDescription = null,
                                        tint = Color.White
                                    )
                                }
                            },
                            content = {
                                DeviceCard(
                                    device = device,
                                    online = false,
                                    onToggle = { controller.toggleAllowTransfer(device.id) },
                                    onClick = null
                                )
                            }
                        )
                    }
                }
            }
        }
    }

    // 删除确认对话框。
    pendingDelete?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingDelete = null },
            title = { Text(stringResource(R.string.delete_device)) },
            text = {
                Text(stringResource(R.string.delete_device_confirm, target.name))
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        controller.removeHistoryDevice(target.id)
                        pendingDelete = null
                    }
                ) {
                    Text(
                        stringResource(R.string.delete_device),
                        color = MaterialTheme.colorScheme.error
                    )
                }
            },
            dismissButton = {
                TextButton(onClick = { pendingDelete = null }) {
                    Text(stringResource(R.string.history_cancel))
                }
            }
        )
    }

    if (showLogSheet) {
        LogSheet(
            logs = LogCollector.logs.collectAsState().value,
            onClear = LogCollector::clear,
            onDismiss = { showLogSheet = false }
        )
    }

    // 设备名称编辑对话框：点击 AppBar 标题弹出，替代原 SettingsScreen。
    // 保存调 controller.setCustomName，controller 内部会同步到 Rust 侧 +
    // 持久化 + 刷新 deviceName StateFlow，UI 标题即时更新。
    if (showNameDialog) {
        DeviceNameDialog(
            currentName = controller.getCustomName(),
            onDismiss = { showNameDialog = false },
            onSave = { name ->
                controller.setCustomName(name)
                showNameDialog = false
            }
        )
    }
}

/**
 * 设备名称 inline 编辑对话框。对齐 Flutter home_page.dart 的 _showDeviceNameDialog。
 *
 * 预填当前自定义名（空串表示用默认名），保存时调 [onSave]；
 * 空串视为重置为默认名 `hyx-{id前6位}`（Rust 侧逻辑）。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DeviceNameDialog(
    currentName: String,
    onDismiss: () -> Unit,
    onSave: (String) -> Unit
) {
    var name by remember { mutableStateOf(currentName) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("设备名称") },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("输入自定义设备名称") },
                leadingIcon = { Icon(Icons.Outlined.Edit, contentDescription = null) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.None,
                    imeAction = ImeAction.Done
                ),
                keyboardActions = KeyboardActions(onDone = { onSave(name) })
            )
        },
        confirmButton = {
            FilledButton(onClick = { onSave(name) }) {
                Text("保存")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text(stringResource(R.string.history_cancel))
            }
        }
    )
}

/** 分区标题：标题 + 数量徽章。对齐 Flutter _SectionHeader。 */
@Composable
private fun SectionHeader(title: String, count: Int) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            title,
            fontSize = 14.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(Modifier.size(8.dp))
        Box(
            modifier = Modifier
                .clip(RoundedCornerShape(10.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHighest)
                .padding(horizontal = 8.dp, vertical = 2.dp)
        ) {
            Text(
                count.toString(),
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

/** 空分区提示。对齐 Flutter _EmptySectionHint。 */
@Composable
private fun EmptySectionHint(text: String) {
    Text(
        text,
        modifier = Modifier.fillMaxWidth().padding(vertical = 12.dp),
        fontSize = 13.sp,
        color = MaterialTheme.colorScheme.outline
    )
}

/** 空状态：扫描中或无设备。对齐 Flutter _EmptyState。 */
@Composable
private fun EmptyState(scanning: Boolean) {
    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Icon(
            imageVector = if (scanning) Icons.Outlined.Radar else Icons.Outlined.DevicesOther,
            contentDescription = null,
            modifier = Modifier.size(72.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f)
        )
        Spacer(Modifier.height(20.dp))
        Text(
            stringResource(if (scanning) R.string.scanning else R.string.no_devices),
            fontSize = 16.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.no_devices_hint),
            fontSize = 13.sp,
            color = MaterialTheme.colorScheme.outline,
            textAlign = TextAlign.Center
        )
        if (scanning) {
            Spacer(Modifier.height(16.dp))
            CircularProgressIndicator(
                modifier = Modifier.size(24.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.primary
            )
        }
    }
}

/**
 * 设备卡片 —— 对齐 Flutter DeviceCard。
 *
 * Column 布局：上半部分 Row(头像 + 名称/地址 + 状态徽章)，下半部分接收开关行。
 * 在线设备正常色调、整卡可点发送；历史设备 50% 透明、不可点击。
 * 内部开关行有自己的 clickable，会消费事件，不会冒泡到卡片点击。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DeviceCard(
    device: Device,
    online: Boolean,
    onToggle: () -> Unit,
    onClick: (() -> Unit)?
) {
    val cardShape = RoundedCornerShape(16.dp)
    val cardColors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)

    if (online && onClick != null) {
        // 在线设备：整卡可点发送。内部开关行自带 clickable 消费事件，不会触发发送。
        Card(
            onClick = onClick,
            modifier = Modifier.fillMaxWidth(),
            shape = cardShape,
            colors = cardColors
        ) {
            DeviceCardContent(device, online, onToggle)
        }
    } else {
        // 历史设备：不可点击。
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = cardShape,
            colors = cardColors
        ) {
            DeviceCardContent(device, online, onToggle)
        }
    }
}

/** 设备卡片内容：上半部分(头像+名称/地址+状态徽章) + 下半部分(接收开关)。 */
@Composable
private fun DeviceCardContent(device: Device, online: Boolean, onToggle: () -> Unit) {
    Column(Modifier.padding(horizontal = 16.dp, vertical = 14.dp)) {
        // 上半部分：头像 + 名称/地址 + 状态徽章。
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.alpha(if (online) 1f else 0.5f)
        ) {
            Avatar(name = device.name)
            Spacer(Modifier.size(14.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    device.name,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurface
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    device.address ?: "",
                    fontSize = 12.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Spacer(Modifier.size(8.dp))
            OnlineStatusBadge(online = online)
        }
        Spacer(Modifier.height(10.dp))
        // 下半部分：接收/禁止开关。
        AllowReceiveToggle(
            allowReceive = device.allowTransfer,
            onChanged = onToggle
        )
    }
}

/** 圆形头像：取名称首字母大写。对齐 Flutter _Avatar。 */
@Composable
private fun Avatar(name: String) {
    val initial = if (name.isEmpty()) "?" else name[0].uppercaseChar().toString()
    Box(
        modifier = Modifier
            .size(44.dp)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center
    ) {
        Text(
            initial,
            fontSize = 18.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}

/** 在线/离线状态徽章：圆点 + 文本。对齐 Flutter _StatusBadge。 */
@Composable
private fun OnlineStatusBadge(online: Boolean) {
    val color = if (online) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
    val bg = if (online) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHighest
    val fg = if (online) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(12.dp))
            .background(bg)
            .padding(horizontal = 8.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier
                .size(6.dp)
                .background(color, CircleShape)
        )
        Spacer(Modifier.size(4.dp))
        Text(
            stringResource(if (online) R.string.online else R.string.offline),
            fontSize = 11.sp,
            color = fg
        )
    }
}

/**
 * 接收/禁止切换行：图标 + 文本 + Switch。对齐 Flutter _AllowReceiveToggle。
 * 点击整行或 Switch 均触发 onChanged；自带 clickable 消费事件，不冒泡到父卡片。
 */
@Composable
private fun AllowReceiveToggle(allowReceive: Boolean, onChanged: () -> Unit) {
    val label = stringResource(if (allowReceive) R.string.allow else R.string.block)
    val iconColor = if (allowReceive) HyxGreen else MaterialTheme.colorScheme.error
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .clickable(onClick = onChanged)
            .padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(
            imageVector = if (allowReceive) Icons.Outlined.CheckCircle else Icons.Outlined.Block,
            contentDescription = null,
            tint = iconColor,
            modifier = Modifier.size(18.dp)
        )
        Spacer(Modifier.size(8.dp))
        Text(
            label,
            fontSize = 13.sp,
            fontWeight = FontWeight.Medium,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(Modifier.weight(1f))
        Switch(
            checked = allowReceive,
            onCheckedChange = { onChanged() }
        )
    }
}

/**
 * 将 content URI 复制到 app cache，使 Rust 内核能按路径读取。
 * 与原实现一致。
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
