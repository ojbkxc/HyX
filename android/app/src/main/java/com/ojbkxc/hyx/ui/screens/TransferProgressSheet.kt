package com.ojbkxc.hyx.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ArrowDownward
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip

import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.ui.components.RingProgress
import com.ojbkxc.hyx.ui.components.formatBytes
import com.ojbkxc.hyx.ui.components.formatDuration
import com.ojbkxc.hyx.ui.components.formatSpeed
import com.ojbkxc.hyx.ui.model.TransferDirection
import com.ojbkxc.hyx.ui.model.TransferProgress
import com.ojbkxc.hyx.ui.model.TransferStatus
import com.ojbkxc.hyx.ui.theme.HyxGreen

/**
 * 传输进度浮层（ModalBottomSheet）—— 对齐 Flutter transfer_progress_sheet.dart。
 *
 * 内容：拖把 + 标题(方向箭头+文件名) + 环形进度 + 状态文本 + 5 指标行 + 取消/关闭按钮。
 * 传输中显示 OutlinedButton 取消；终态显示 Button 关闭。
 *
 * @param progress 当前传输进度快照
 * @param status   当前传输状态
 * @param onCancel 取消传输回调
 * @param onDismiss 关闭浮层回调
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TransferProgressSheet(
    progress: TransferProgress,
    status: TransferStatus,
    onCancel: () -> Unit,
    onDismiss: () -> Unit
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        dragHandle = null
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 24.dp, end = 24.dp, top = 20.dp, bottom = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            // 1. 拖把。
            Box(
                modifier = Modifier
                    .width(40.dp)
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(MaterialTheme.colorScheme.outlineVariant)
            )
            Spacer(Modifier.height(16.dp))

            // 2. 标题：方向箭头 + 文件名。
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center
            ) {
                Icon(
                    imageVector = if (progress.direction == TransferDirection.Send)
                        Icons.Outlined.ArrowUpward else Icons.Outlined.ArrowDownward,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(18.dp)
                )
                Spacer(Modifier.size(6.dp))
                Text(
                    if (progress.name.isEmpty()) stringResource(R.string.transfer_in_progress) else progress.name,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
            }
            Spacer(Modifier.height(20.dp))

            // 4. 环形进度。
            RingProgress(progress = progress.fraction, size = 160.dp)
            Spacer(Modifier.height(20.dp))

            // 6. 状态文本。
            StatusText(status = status)
            Spacer(Modifier.height(16.dp))

            // 8. 指标行：已传 / 总量 / 速度 / 已用时 / 剩余。
            MetricRow(progress = progress)
            Spacer(Modifier.height(20.dp))

            // 10. 取消/关闭按钮。
            val busy = status == TransferStatus.Pairing ||
                status == TransferStatus.Connecting ||
                status == TransferStatus.Transferring
            if (busy) {
                OutlinedButton(onClick = onCancel) {
                    Icon(Icons.Outlined.Close, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.size(6.dp))
                    Text(stringResource(R.string.cancel))
                }
            } else {
                Button(onClick = onDismiss) {
                    Text(stringResource(R.string.transfer_close))
                }
            }
        }
    }
}

/** 状态文本：根据 status 显示不同颜色。对齐 Flutter _StatusText。 */
@Composable
private fun StatusText(status: TransferStatus) {
    val scheme = MaterialTheme.colorScheme
    val (text, color) = when (status) {
        TransferStatus.Idle -> stringResource(R.string.transfer_idle) to scheme.onSurfaceVariant
        TransferStatus.Pairing -> stringResource(R.string.transfer_pairing) to scheme.tertiary
        TransferStatus.Connecting -> stringResource(R.string.transfer_connecting) to scheme.tertiary
        TransferStatus.Transferring -> stringResource(R.string.transfer_transferring) to scheme.primary
        TransferStatus.Completed -> stringResource(R.string.transfer_completed) to HyxGreen
        TransferStatus.Failed -> stringResource(R.string.transfer_failed) to scheme.error
        TransferStatus.Cancelled -> stringResource(R.string.transfer_cancelled) to scheme.onSurfaceVariant
    }
    Text(
        text,
        fontSize = 14.sp,
        fontWeight = FontWeight.Medium,
        color = color,
        textAlign = TextAlign.Center,
        maxLines = 2,
        overflow = TextOverflow.Ellipsis
    )
}

/** 指标行：已传 / 总量 / 速度 / 已用时 / 剩余。对齐 Flutter _MetricRow。 */
@Composable
private fun MetricRow(progress: TransferProgress) {
    val labelStyle = androidx.compose.ui.text.TextStyle(
        fontSize = 11.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant
    )
    val valueStyle = androidx.compose.ui.text.TextStyle(
        fontSize = 14.sp,
        fontWeight = FontWeight.SemiBold
    )
    // ETA: 剩余字节 / 速度 * 1000ms。
    val etaMs = if (progress.speedBps > 0 && progress.totalBytes > progress.transferredBytes) {
        ((progress.totalBytes - progress.transferredBytes) / progress.speedBps * 1000).toLong()
    } else 0L

    Row(modifier = Modifier.fillMaxWidth()) {
        Metric(
            label = stringResource(R.string.transfer_transferred),
            value = formatBytes(progress.transferredBytes),
            labelStyle = labelStyle,
            valueStyle = valueStyle
        )
        Metric(
            label = stringResource(R.string.transfer_total),
            value = formatBytes(progress.totalBytes),
            labelStyle = labelStyle,
            valueStyle = valueStyle
        )
        Metric(
            label = stringResource(R.string.transfer_speed),
            value = formatSpeed(progress.speedBps),
            labelStyle = labelStyle,
            valueStyle = valueStyle
        )
        Metric(
            label = stringResource(R.string.transfer_elapsed),
            value = formatDuration(progress.elapsedMs),
            labelStyle = labelStyle,
            valueStyle = valueStyle
        )
        Metric(
            label = stringResource(R.string.transfer_remaining),
            value = if (etaMs > 0) formatDuration(etaMs) else "—",
            labelStyle = labelStyle,
            valueStyle = valueStyle
        )
    }
}

/** 单个指标列（等宽）。对齐 Flutter _Metric。 */
@Composable
private fun Metric(
    label: String,
    value: String,
    labelStyle: androidx.compose.ui.text.TextStyle,
    valueStyle: androidx.compose.ui.text.TextStyle
) {
    Column(
        modifier = Modifier.weight(1f),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(label, style = labelStyle)
        Spacer(Modifier.height(2.dp))
        Text(value, style = valueStyle, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}