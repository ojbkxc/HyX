package com.ojbkxc.hyx.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ojbkxc.hyx.ui.components.MetricCardRow
import com.ojbkxc.hyx.ui.components.RingProgress
import com.ojbkxc.hyx.ui.components.StatusBadge
import com.ojbkxc.hyx.ui.components.formatBytes
import com.ojbkxc.hyx.ui.components.formatDuration
import com.ojbkxc.hyx.ui.components.formatSpeed
import com.ojbkxc.hyx.ui.model.TransferDirection
import com.ojbkxc.hyx.ui.model.TransferStatus
import com.ojbkxc.hyx.core.HyXCoreController

@Composable
fun TransferScreen(controller: HyXCoreController, onScan: () -> Unit) {
    val status by controller.status.collectAsState()
    val direction by controller.direction.collectAsState()
    val progress by controller.progress.collectAsState()
    val pairingCode by controller.pairingCode.collectAsState()
    val settings by controller.settings.collectAsState()

    val active = status == TransferStatus.Transferring ||
        status == TransferStatus.Connecting || status == TransferStatus.Pairing

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(
            "极速传输",
            style = MaterialTheme.typography.headlineSmall,
            color = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.align(Alignment.Start)
        )

        Spacer(Modifier.height(16.dp))

        // 发送 / 接收 segmented toggle
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            SegmentedButton(
                selected = direction == TransferDirection.Send,
                onClick = { if (!active) controller.onDirectionChange(TransferDirection.Send) },
                shape = SegmentedButtonDefaults.itemShape(0, 2)
            ) { Text("发送") }
            SegmentedButton(
                selected = direction == TransferDirection.Receive,
                onClick = { if (!active) controller.onDirectionChange(TransferDirection.Receive) },
                shape = SegmentedButtonDefaults.itemShape(1, 2)
            ) { Text("接收") }
        }

        Spacer(Modifier.height(16.dp))

        if (pairingCode != null && !active) {
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
                shape = RoundedCornerShape(16.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text("配对码", fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(6.dp))
                    Text(
                        pairingCode!!.code,
                        style = MaterialTheme.typography.headlineSmall,
                        color = MaterialTheme.colorScheme.onSurface,
                        letterSpacing = 4.sp,
                        fontWeight = FontWeight.Bold
                    )
                }
            }
            Spacer(Modifier.height(16.dp))
        }

        if (active && progress != null) {
            TransferringPanel(progress = progress!!, onCancel = controller::cancelTransfer)
        } else {
            TransferIdlePanel(
                hasPairing = pairingCode != null,
                onStart = controller::startTransfer,
                onPair = { controller.pairWithCode(pairingCode?.code ?: "HYX-" + (1000..9999).random()) },
                onScan = onScan,
                onStartDiscovery = controller::startDiscovery
            )
        }

        Spacer(Modifier.height(20.dp))

        EngineSettingsCard(settings = settings, onChange = controller::updateSettings)

        Spacer(Modifier.height(8.dp))
        StatusBadge(text = statusText(status), active = active)
        Spacer(Modifier.height(4.dp))
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
                "总大小" to formatBytes(progress.totalBytes),
                "已传输" to formatBytes(progress.transferredBytes),
                "速度" to formatSpeed(progress.speedBps),
                "剩余" to formatDuration(etaMs(progress))
            )
        )
        Spacer(Modifier.height(20.dp))
        OutlinedButton(onClick = onCancel) { Text("取消") }
    }
}

@Composable
private fun TransferIdlePanel(
    hasPairing: Boolean,
    onStart: () -> Unit,
    onPair: () -> Unit,
    onScan: () -> Unit,
    onStartDiscovery: () -> Unit
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        Button(
            onClick = onStart,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(16.dp)
        ) { Text("选择文件并开始传输") }

        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            OutlinedButton(
                onClick = onPair,
                modifier = Modifier.weight(1f)
            ) { Text("配对码") }
            OutlinedButton(
                onClick = onScan,
                modifier = Modifier.weight(1f)
            ) { Text("扫码") }
        }
        OutlinedButton(
            onClick = onStartDiscovery,
            modifier = Modifier.fillMaxWidth()
        ) { Text("局域网发现", color = MaterialTheme.colorScheme.primary) }
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
            Text("引擎", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(12.dp))
            SettingSwitch("流式压缩（zstd，自适应）", settings.compression) {
                onChange(settings.copy(compression = it))
            }
            SettingSwitch("连续流聚合（一次写整批）", settings.aggregation) {
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

private fun statusText(status: TransferStatus): String = when (status) {
    TransferStatus.Idle -> "待命"
    TransferStatus.Pairing -> "等待配对…"
    TransferStatus.Connecting -> "建立连接…"
    TransferStatus.Transferring -> "传输中"
    TransferStatus.Completed -> "已完成"
    TransferStatus.Failed -> "失败"
    TransferStatus.Cancelled -> "已取消"
}

private fun etaMs(p: com.ojbkxc.hyx.ui.model.TransferProgress): Long {
    if (p.speedBps <= 0) return 0
    val remaining = (p.totalBytes - p.transferredBytes).coerceAtLeast(0)
    return (remaining / p.speedBps * 1000).toLong()
}