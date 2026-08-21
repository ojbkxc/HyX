package com.ojbkxc.hyx.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.weight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ojbkxc.hyx.ui.theme.HyxGreen

/**
 * Symmetric dashboard card row: N metric cards of identical size, aligned at
 * the base line, each labelled + big value. Keeps the layout clean without a
 * stray progress bar (progress lives in the ring).
 */
@Composable
fun MetricCardRow(
    metrics: List<Pair<String, String>>,
    modifier: Modifier = Modifier,
    cardHeight: Dp = 76.dp
) {
    Row(modifier = modifier.fillMaxWidth()) {
        metrics.forEachIndexed { i, (label, value) ->
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = if (i == 0) 0.dp else 6.dp, end = if (i == metrics.lastIndex) 0.dp else 6.dp)
            ) {
                MetricCard(label, value, height = cardHeight)
            }
        }
    }
}

@Composable
fun MetricCard(label: String, value: String, modifier: Modifier = Modifier, height: Dp = 76.dp) {
    Column(
        modifier = modifier
            .height(height)
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(16.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp)
    ) {
        Text(label, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.weight(1f))
        Text(
            value,
            fontSize = 15.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurface,
            maxLines = 1
        )
    }
}

/** Accent chip for status / direction labels. */
@Composable
fun StatusBadge(text: String, modifier: Modifier = Modifier, active: Boolean = false) {
    val bg = if (active) HyxGreen.copy(alpha = 0.18f) else MaterialTheme.colorScheme.surfaceVariant
    val fg = if (active) HyxGreen else MaterialTheme.colorScheme.onSurfaceVariant
    Text(
        text,
        modifier = modifier
            .background(bg, RoundedCornerShape(50))
            .padding(horizontal = 10.dp, vertical = 4.dp),
        fontSize = 11.sp,
        fontWeight = FontWeight.Medium,
        color = fg
    )
}