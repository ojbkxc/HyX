package com.ojbkxc.hyx.ui.components

import java.util.Locale

/** Human-readable byte size, e.g. 3.2 GB. */
fun formatBytes(bytes: Long): String {
    if (bytes <= 0) return "0 B"
    val unit = 1024.0
    val units = arrayOf("B", "KB", "MB", "GB", "TB")
    var v = bytes.toDouble()
    var i = 0
    while (v >= unit && i < units.lastIndex) { v /= unit; i++ }
    return if (i == 0) "${bytes} B" else String.format(Locale.US, "%.1f %s", v, units[i])
}

/** e.g. 12.3 MB/s */
fun formatSpeed(bytesPerSec: Double): String =
    if (bytesPerSec <= 0) "—" else "${formatBytes(bytesPerSec.toLong())}/s"

/** mm:ss or h:mm:ss */
fun formatDuration(millis: Long): String {
    val totalSec = millis / 1000
    val h = totalSec / 3600
    val m = (totalSec % 3600) / 60
    val s = totalSec % 60
    return if (h > 0) String.format(Locale.US, "%d:%02d:%02d", h, m, s)
        else String.format(Locale.US, "%02d:%02d", m, s)
}

/** Best-effort MIME type from a file extension; defaults to application/octet-stream. */
fun mimeTypeOf(name: String): String {
    val ext = name.substringAfterLast('.', "").lowercase(Locale.US)
    return when (ext) {
        "mp4", "mkv", "webm", "mov", "avi" -> "video/$ext"
        "mp3", "wav", "flac", "ogg", "m4a", "aac" -> "audio/$ext"
        "jpg", "jpeg" -> "image/jpeg"
        "png" -> "image/png"
        "gif" -> "image/gif"
        "webp" -> "image/webp"
        "bmp" -> "image/bmp"
        "heic", "heif" -> "image/heic"
        "pdf" -> "application/pdf"
        "zip", "gz", "7z", "rar" -> "application/zip"
        "apk" -> "application/vnd.android.package-archive"
        "doc", "docx" -> "application/msword"
        "xls", "xlsx" -> "application/vnd.ms-excel"
        "ppt", "pptx" -> "application/vnd.ms-powerpoint"
        "txt", "md", "log", "json", "xml", "csv", "html", "htm" -> "text/plain"
        else -> "application/octet-stream"
    }
}