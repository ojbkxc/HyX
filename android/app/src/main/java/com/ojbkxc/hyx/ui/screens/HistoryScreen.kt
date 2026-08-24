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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.History
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.core.HyXCoreController
import com.ojbkxc.hyx.ui.components.formatBytes
import com.ojbkxc.hyx.ui.components.formatDuration
import com.ojbkxc.hyx.ui.model.HistoryRecord
import com.ojbkxc.hyx.ui.model.TransferDirection
import com.ojbkxc.hyx.ui.model.TransferStatus
import com.ojbkxc.hyx.ui.theme.HyxAmber
import com.ojbkxc.hyx.ui.theme.HyxGreen
import com.ojbkxc.hyx.ui.theme.HyxRed
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * 历史页 —— 对齐 Flutter history_drawer.dart。
 *
 * 头部：标题 + 数量 + 清空按钮（非空时）。
 * 列表：每条记录显示方向圆形图标、文件名、状态徽章、大小/地址/耗时、时间戳。
 */
@Composable
fun HistoryScreen(controller: HyXCoreController) {
    val history by controller.history.collectAsState()
    var showClearConfirm by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().padding(horizontal = 20.dp, vertical = 12.dp)) {
        // 头部：标题 + 清空按钮。
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(
                Icons.Outlined.History,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(22.dp)
            )
            Spacer(Modifier.size(10.dp))
            Text(
                stringResource(R.string.tab_history_title),
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.weight(1f)
            )
            if (history.isNotEmpty()) {
                IconButton(onClick = { showClearConfirm = true }) {
                    Icon(
                        Icons.Outlined.DeleteSweep,
                        contentDescription = stringResource(R.string.history_clear),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
        }
        Spacer(Modifier.height(12.dp))
        Text(
            stringResource(R.string.history_count, history.size),
            fontSize = 13.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(Modifier.height(12.dp))

        if (history.isEmpty()) {
            Column(
                Modifier.fillMaxSize(),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                Icon(
                    Icons.Outlined.History,
                    contentDescription = null,
                    modifier = Modifier.size(48.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f)
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(R.string.history_empty),
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        } else {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                items(history, key = { it.id }) { record ->
                    HistoryCard(record)
                }
            }
        }
    }

    // 清空确认对话框。
    if (showClearConfirm) {
        AlertDialog(
            onDismissRequest = { showClearConfirm = false },
            title = { Text(stringResource(R.string.history_clear)) },
            text = { Text(stringResource(R.string.history_clear_confirm)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        controller.clearHistory()
                        showClearConfirm = false
                    }
                ) {
                    Text(stringResource(R.string.history_clear))
                }
            },
            dismissButton = {
                TextButton(onClick = { showClearConfirm = false }) {
                    Text(stringResource(R.string.history_cancel))
                }
            }
        )
    }
}

/** 历史卡片 —— 对齐 Flutter _HistoryCard。 */
@Composable
private fun HistoryCard(record: HistoryRecord) {
    val statusColor = when (record.status) {
        TransferStatus.Completed -> HyxGreen
        TransferStatus.Failed -> HyxRed
        TransferStatus.Cancelled -> MaterialTheme.colorScheme.onSurfaceVariant
        else -> HyxAmber
    }
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            // 方向圆形图标。
            Column(
                modifier = Modifier
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(statusColor.copy(alpha = 0.15f)),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                Text(
                    if (record.direction == TransferDirection.Send) "↑" else "↓",
                    color = statusColor,
                    fontWeight = FontWeight.Bold,
                    fontSize = 18.sp
                )
            }
            Spacer(Modifier.size(12.dp))
            // 文件信息。
            Column(Modifier.weight(1f)) {
                Text(
                    record.name,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurface
                )
                Spacer(Modifier.height(3.dp))
                Text(
                    "${formatBytes(record.bytesTransferred)} · ${record.peerAddress} · ${formatDuration(record.durationSecs * 1000)}",
                    fontSize = 11.sp,
                    maxLines = 1,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    SimpleDateFormat("MM-dd HH:mm", Locale.getDefault()).format(Date(record.timestamp)),
                    fontSize = 10.sp,
                    color = MaterialTheme.colorScheme.outline
                )
            }
            Spacer(Modifier.size(8.dp))
            // 状态徽章：彩色 pill。
            HistoryStatusBadge(status = record.status)
        }
    }
}

/**
 * 状态徽章 —— 对齐 Flutter _StatusBadge。
 * completed=绿/failed=红/cancelled=灰/interrupted=琥珀，文字白/深色。
 */
@Composable
private fun HistoryStatusBadge(status: TransferStatus) {
    val (label, color, fg) = when (status) {
        TransferStatus.Completed -> Triple(stringResource(R.string.transfer_completed), HyxGreen, Color.White)
        TransferStatus.Failed -> Triple(stringResource(R.string.transfer_failed), MaterialTheme.colorScheme.error, Color.White)
        TransferStatus.Cancelled -> Triple(stringResource(R.string.transfer_cancelled), MaterialTheme.colorScheme.outline, MaterialTheme.colorScheme.onSurface)
        else -> Triple(stringResource(R.string.status_interrupted), HyxAmber, Color.White)
    }
    Text(
        label,
        modifier = Modifier
            .clip(RoundedCornerShape(10.dp))
            .background(color)
            .padding(horizontal = 8.dp, vertical = 3.dp),
        fontSize = 10.sp,
        fontWeight = FontWeight.Medium,
        color = fg
    )
}
