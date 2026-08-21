package com.ojbkxc.hyx.ui.components

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.ojbkxc.hyx.ui.theme.HyxGreen
import com.ojbkxc.hyx.ui.theme.Slate300

/**
 * Minimal ring progress. Value animates automatically; the track is a thin
 * slate ring, the sweep is a full-round green arc with a round cap.
 */
@Composable
fun RingProgress(
    progress: Float,
    modifier: Modifier = Modifier,
    size: Dp = 140.dp,
    trackThickness: Dp = 10.dp,
    progressThickness: Dp = 10.dp
) {
    val animated by animateFloatAsState(targetValue = progress.coerceIn(0f, 1f))
    Canvas(modifier = modifier) {
        val strokeWidth = progressThickness.toPx()
        val gap = 3.dp.toPx()
        val radius = (size.toPx() - strokeWidth) / 2f - gap
        val trackStroke = Stroke(width = trackThickness.toPx(), cap = StrokeCap.Round)
        val startAngle = -90f
        val sweep = animated * 360f
        drawCircle(color = Slate300.copy(alpha = 0.35f), radius = radius + strokeWidth,
            style = Stroke(width = trackThickness.toPx()))
        drawArc(
            color = HyxGreen,
            startAngle = startAngle,
            sweepAngle = sweep,
            useCenter = false,
            style = Stroke(width = strokeWidth, cap = StrokeCap.Round)
        )
    }
}