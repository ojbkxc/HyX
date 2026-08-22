import 'dart:math' as math;

/// 人类可读的字节大小，例如 `3.2 GB`。
///
/// 对应 Kotlin `Formatters.formatBytes`：1024 进制，B 整数显示，其余保留 1 位小数。
String formatBytes(int bytes) {
  if (bytes <= 0) return '0 B';
  const unit = 1024.0;
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var v = bytes.toDouble();
  var i = 0;
  while (v >= unit && i < units.length - 1) {
    v /= unit;
    i++;
  }
  if (i == 0) return '$bytes B';
  return '${v.toStringAsFixed(1)} ${units[i]}';
}

/// 速率格式化，例如 `12.3 MB/s`。`<=0` 时返回 `—` 表示无速率。
String formatSpeed(double bytesPerSec) {
  if (bytesPerSec <= 0) return '—';
  return '${formatBytes(bytesPerSec.toInt())}/s';
}

/// 时长格式化：`mm:ss` 或 `h:mm:ss`。
String formatDuration(Duration d) {
  final totalSec = d.inSeconds;
  final h = totalSec ~/ 3600;
  final m = (totalSec % 3600) ~/ 60;
  final s = totalSec % 60;
  if (h > 0) {
    return '$h:${m.toString().padLeft(2, '0')}:${s.toString().padLeft(2, '0')}';
  }
  return '${m.toString().padLeft(2, '0')}:${s.toString().padLeft(2, '0')}';
}

/// 根据当前速率与剩余字节估算 ETA（毫秒）。无速率或已超时返回 0。
int etaMillis(int transferred, int total, double speedBps) {
  if (speedBps <= 0) return 0;
  final remaining = math.max(0, total - transferred);
  return (remaining / speedBps * 1000).toInt();
}

/// 简单 MIME 类型推断，默认 `application/octet-stream`。
///
/// 对应 Kotlin `mimeTypeOf`：仅按扩展名映射常见音视频/图片/文档/压缩包。
String mimeTypeOf(String name) {
  final ext = name.contains('.') ? name.substring(name.lastIndexOf('.') + 1).toLowerCase() : '';
  switch (ext) {
    case 'mp4':
    case 'mkv':
    case 'webm':
    case 'mov':
    case 'avi':
      return 'video/$ext';
    case 'mp3':
    case 'wav':
    case 'flac':
    case 'ogg':
    case 'm4a':
    case 'aac':
      return 'audio/$ext';
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg';
    case 'png':
      return 'image/png';
    case 'gif':
      return 'image/gif';
    case 'webp':
      return 'image/webp';
    case 'bmp':
      return 'image/bmp';
    case 'heic':
    case 'heif':
      return 'image/heic';
    case 'pdf':
      return 'application/pdf';
    case 'zip':
    case 'gz':
    case '7z':
    case 'rar':
      return 'application/zip';
    case 'apk':
      return 'application/vnd.android.package-archive';
    case 'doc':
    case 'docx':
      return 'application/msword';
    case 'xls':
    case 'xlsx':
      return 'application/vnd.ms-excel';
    case 'ppt':
    case 'pptx':
      return 'application/vnd.ms-powerpoint';
    case 'txt':
    case 'md':
    case 'log':
    case 'json':
    case 'xml':
    case 'csv':
    case 'html':
    case 'htm':
      return 'text/plain';
    default:
      return 'application/octet-stream';
  }
}