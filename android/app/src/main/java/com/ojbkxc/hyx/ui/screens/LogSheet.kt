package com.ojbkxc.hyx.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.background
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Article
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.FileDownload
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
import com.ojbkxc.hyx.R
import com.ojbkxc.hyx.ui.model.LogEntry
import com.ojbkxc.hyx.ui.model.LogLevel
import com.ojbkxc.hyx.ui.model.LogSource
import com.ojbkxc.hyx.ui.theme.HyxAmber
import com.ojbkxc.hyx.ui.theme.HyxBlue
import com.ojbkxc.hyx.ui.theme.HyxGreen
import com.ojbkxc.hyx.ui.theme.HyxRed
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * In-app log viewer shown as a [ModalBottomSheet]. Renders the unified Rust +
 * Android log stream collected by [com.ojbkxc.hyx.core.LogCollector].
 *
 * Features:
 *  - Level filter chips (All / Error / Warn / Info / Debug).
 *  - Copy all logs to clipboard.
 *  - Clear all logs.
 *  - Export logs to a user-chosen .txt file via SAF [ActivityResultContracts.CreateDocument].
 *
 * @param logs     snapshot list of log entries to display
 * @param onClear  invoked when the user taps the clear button
 * @param onDismiss invoked when the sheet is dismissed (swipe or scrim tap)
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LogSheet(
    logs: List<LogEntry>,
    onClear: () -> Unit,
    onDismiss: () -> Unit
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val context = LocalContext.current

    // Active level filter: null = all, otherwise show only that level and above.
    var filterLevel by remember { mutableStateOf<LogLevel?>(null) }

    // Toast helper.
    fun toast(resId: Int) = Toast.makeText(context, resId, Toast.LENGTH_SHORT).show()

    // SAF launcher for export: writes the joined log text into the chosen file.
    val exportLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("text/plain")
    ) { uri ->
        if (uri != null) {
            val text = logs.joinToString("\n") { it.formatted() }
            val ok = runCatching {
                context.contentResolver.openOutputStream(uri)?.use { os ->
                    os.write(text.toByteArray())
                }
            }.isSuccess
            toast(if (ok) R.string.log_exported else R.string.log_export_failed)
        }
    }

    val filtered = remember(logs, filterLevel) {
        if (filterLevel == null) logs else logs.filter { it.level.ordinal >= filterLevel!!.ordinal }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState
    ) {
        LogSheetHeader(
            onCopy = {
                val text = logs.joinToString("\n") { it.formatted() }
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText("HyX log", text))
                toast(R.string.log_copied)
            },
            onClear = {
                onClear()
                toast(R.string.log_cleared)
            },
            onExport = {
                if (logs.isEmpty()) {
                    toast(R.string.log_empty)
                } else {
                    exportLauncher.launch("hyx-log-${System.currentTimeMillis()}.txt")
                }
            }
        )

        LevelFilterRow(
            active = filterLevel,
            onSelect = { filterLevel = it }
        )

        if (filtered.isEmpty()) {
            Text(
                stringResource(R.string.log_empty),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 32.dp),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .fillMaxHeight(0.85f)
                    .padding(horizontal = 16.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                items(filtered) { entry -> LogRow(entry) }
            }
        }
        Spacer(Modifier.height(12.dp))
    }
}

@Composable
private fun LogSheetHeader(
    onCopy: () -> Unit,
    onClear: () -> Unit,
    onExport: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(
            Icons.Outlined.Article,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.primary,
            modifier = Modifier.size(20.dp)
        )
        Spacer(Modifier.size(8.dp))
        Text(
            stringResource(R.string.log_title),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold
        )
        Spacer(Modifier.weight(1f))
        IconButton(onClick = onCopy) {
            Icon(Icons.Outlined.ContentCopy, contentDescription = stringResource(R.string.log_copy))
        }
        IconButton(onClick = onExport) {
            Icon(Icons.Outlined.FileDownload, contentDescription = stringResource(R.string.log_export))
        }
        IconButton(onClick = onClear) {
            Icon(Icons.Outlined.DeleteSweep, contentDescription = stringResource(R.string.log_clear))
        }
    }
}

@Composable
private fun LevelFilterRow(
    active: LogLevel?,
    onSelect: (LogLevel?) -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        FilterChip(
            selected = active == null,
            onClick = { onSelect(null) },
            label = { Text(stringResource(R.string.log_filter_all)) }
        )
        LogLevel.entries.forEach { level ->
            FilterChip(
                selected = active == level,
                onClick = { onSelect(if (active == level) null else level) },
                label = { Text(level.name) }
            )
        }
    }
}

@Composable
private fun LogRow(entry: LogEntry) {
    val time = remember(entry.timestamp) {
        SimpleDateFormat("HH:mm:ss.SSS", Locale.US).format(Date(entry.timestamp))
    }
    val levelColor = levelColor(entry.level)
    val sourceColor = if (entry.source == LogSource.Rust) HyxBlue else HyxGreen

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface, RoundedCornerShape(6.dp))
            .padding(horizontal = 8.dp, vertical = 4.dp)
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                time,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Spacer(Modifier.size(6.dp))
            Text(
                "[${entry.level.name.uppercase()}]",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = levelColor
            )
            Spacer(Modifier.size(6.dp))
            Text(
                "[${entry.source.name}]",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                color = sourceColor
            )
            Spacer(Modifier.size(6.dp))
            Text(
                entry.tag,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1
            )
        }
        Text(
            entry.message,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.fillMaxWidth()
        )
    }
}

@Composable
private fun levelColor(level: LogLevel): Color = when (level) {
    LogLevel.Trace -> MaterialTheme.colorScheme.onSurfaceVariant
    LogLevel.Debug -> MaterialTheme.colorScheme.onSurfaceVariant
    LogLevel.Info -> HyxGreen
    LogLevel.Warn -> HyxAmber
    LogLevel.Error -> HyxRed
}