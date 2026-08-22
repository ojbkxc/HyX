package com.ojbkxc.hyx.ui.components

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.google.zxing.BarcodeFormat
import com.google.zxing.MultiFormatWriter
import com.google.zxing.WriterException

/**
 * QR code Composable. Encodes [content] into a QR-code [Bitmap] via zxing's
 * [MultiFormatWriter], then renders it as a Compose [Image].
 *
 * - Generation is cached with `remember(content, px)` so recomposition with
 *   unchanged content does not re-encode.
 * - On [WriterException] (e.g. content too long for the requested size) a
 *   small placeholder box is shown instead of crashing.
 * - The QR is rendered on a white background so it scans reliably under both
 *   light and dark themes (scanners expect dark-on-light modules).
 *
 * @param content  text to encode (e.g. `"hyx://pair/HYX-AB12CD"`)
 * @param modifier outer modifier
 * @param size     square edge length in dp (default 200.dp)
 */
@Composable
fun QrCode(
    content: String,
    modifier: Modifier = Modifier,
    size: Dp = 200.dp
) {
    val px = with(LocalDensity.current) { size.roundToPx() }
    val bitmap = remember(content, px) { encodeQr(content, px) }

    if (bitmap != null) {
        Image(
            bitmap = bitmap.asImageBitmap(),
            contentDescription = "QR code",
            modifier = modifier.size(size),
            contentScale = ContentScale.Fit
        )
    } else {
        // Fallback when encoding fails (content empty / too long).
        Box(
            modifier = modifier
                .size(size)
                .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(8.dp)),
            contentAlignment = Alignment.Center
        ) {
            Text("N/A", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Encode [content] as a QR code; returns null on [WriterException]. */
private fun encodeQr(content: String, px: Int): Bitmap? {
    if (content.isEmpty() || px <= 0) return null
    return try {
        val matrix = MultiFormatWriter().encode(content, BarcodeFormat.QR_CODE, px, px)
        matrixToBitmap(matrix)
    } catch (_: WriterException) {
        null
    } catch (_: IllegalArgumentException) {
        null
    }
}

/** BitMatrix → Bitmap. Dark module = 0xFF000000, light = 0xFFFFFFFF. */
private fun matrixToBitmap(matrix: com.google.zxing.common.BitMatrix): Bitmap {
    val w = matrix.width
    val h = matrix.height
    val pixels = IntArray(w * h)
    for (y in 0 until h) {
        val offset = y * w
        for (x in 0 until w) {
            pixels[offset + x] = if (matrix[x, y]) 0xFF000000.toInt() else 0xFFFFFFFF.toInt()
        }
    }
    return Bitmap.createBitmap(pixels, w, h, Bitmap.Config.ARGB_8888)
}
