package com.ojbkxc.hyx.ui.components

import android.graphics.Bitmap
import android.graphics.Color
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter

/** Render [content] as a crisp QR code. Pure in-app generation via ZXing core
 *  (no camera involved here) so the peer can scan it. */
@Composable
fun QrCode(content: String, size: Dp = 180.dp) {
    val bmp = remember(content) { qrBitmap(content) }
    if (bmp != null) {
        Image(
            bitmap = bmp.asImageBitmap(),
            contentDescription = null,
            modifier = Modifier
                .size(size)
                .clip(RoundedCornerShape(12.dp))
                .background(Color.White)
        )
    }
}

private fun qrBitmap(content: String, sizePx: Int = 768): Bitmap? = try {
    val hints = java.util.HashMap<EncodeHintType, Any>().apply {
        put(EncodeHintType.CHARACTER_SET, "UTF-8")
        put(EncodeHintType.MARGIN, 1)
    }
    val matrix = QRCodeWriter().encode(content, BarcodeFormat.QR_CODE, sizePx, sizePx, hints)
    val px = IntArray(sizePx * sizePx)
    for (y in 0 until sizePx) {
        for (x in 0 until sizePx) {
            val dark = matrix.get(x, y)
            px[y * sizePx + x] = if (dark) Color.BLACK else Color.WHITE
        }
    }
    Bitmap.createBitmap(px, sizePx, sizePx, Bitmap.Config.ARGB_8888)
} catch (e: Exception) {
    null
}