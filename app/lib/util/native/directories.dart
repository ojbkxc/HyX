import 'dart:io' show Directory, Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart' as path;

const _methodChannel = MethodChannel('com.ojbkxc.hyx_app/hyx');

/// 获取默认接收目录（Downloads 文件夹），跨平台。
///
/// 对齐 localsend 的 `getDefaultDestinationDirectory`：
/// - Android：通过 Method Channel 获取系统 Downloads 目录，回退到 `/storage/emulated/0/Download`
/// - iOS：`getApplicationDocumentsDirectory()`（iOS 没有公共 Downloads）
/// - Windows/macOS/Linux：`path.getDownloadsDirectory()`，回退到 `$HOME/Downloads` 或 `$HOMEPATH/Downloads`
///
/// 与 localsend 不同的是不创建 HyX 子目录，直接存到 Downloads 根目录，
/// 与用户预期一致（手机/电脑接收的文件都在 Downloads 里）。
Future<String> getDefaultDownloadDirectory() async {
  switch (defaultTargetPlatform) {
    case TargetPlatform.android:
      try {
        final dir = await _methodChannel.invokeMethod<String>('getDownloadsDirectory');
        if (dir != null && dir.isNotEmpty) return dir;
      } catch (_) {}
      return '/storage/emulated/0/Download';
    case TargetPlatform.iOS:
      return (await path.getApplicationDocumentsDirectory()).path;
    case TargetPlatform.linux:
    case TargetPlatform.macOS:
    case TargetPlatform.windows:
    case TargetPlatform.fuchsia:
      var downloadDir = await path.getDownloadsDirectory();
      if (downloadDir == null) {
        if (defaultTargetPlatform == TargetPlatform.windows) {
          final homePath = Platform.environment['HOMEPATH'] ?? Platform.environment['USERPROFILE'] ?? '';
          downloadDir = Directory('$homePath\\Downloads');
          if (!downloadDir.existsSync()) {
            downloadDir = Directory(homePath);
          }
        } else {
          final home = Platform.environment['HOME'] ?? '';
          downloadDir = Directory('$home/Downloads');
          if (!downloadDir.existsSync()) {
            downloadDir = Directory(home);
          }
        }
      }
      return downloadDir.path.replaceAll('\\', '/');
  }
}

/// 获取缓存目录（用于临时文件）。
Future<String> getCacheDirectory() async {
  final dir = await path.getTemporaryDirectory();
  await dir.create(recursive: true);
  return dir.path;
}