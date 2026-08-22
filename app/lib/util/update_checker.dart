import 'dart:convert';
import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:http/http.dart' as http;

/// 更新信息
class UpdateInfo {
  final String version;
  final String url;
  final String body;

  const UpdateInfo({
    required this.version,
    required this.url,
    required this.body,
  });
}

/// 多平台更新检测器
///
/// 检测顺序：
/// 1. 自定义下载站 https://downloads.lxseek.com/HyX/latest.json（优先）
/// 2. GitHub releases（回退，过滤 prerelease）
class UpdateChecker {
  static const _customBaseUrl = 'https://downloads.lxseek.com/HyX';
  static const _githubApi =
      'https://api.github.com/repos/ojbkxc/HyX/releases?per_page=10';

  /// 检测更新，返回 [UpdateInfo] 或 null
  static Future<UpdateInfo?> check(String currentVersion) async {
    // 1. 优先检测自定义下载站
    try {
      final response = await http.get(Uri.parse('$_customBaseUrl/latest.json'));
      if (response.statusCode == 200) {
        final data = json.decode(response.body) as Map<String, dynamic>;
        final version = data['version'] as String;
        if (_compareVersions(version, currentVersion) > 0) {
          final files = data['files'] as Map<String, dynamic>;
          final platformKey = await _getPlatformKey();
          final fileName = files[platformKey] as String?;
          if (fileName != null) {
            return UpdateInfo(
              version: version,
              url: '$_customBaseUrl/$fileName',
              body: data['body'] as String? ?? '',
            );
          }
        }
        return null; // 版本不比当前新
      }
    } catch (_) {
      // 自定义下载站检测失败，回退到 GitHub
    }

    // 2. 回退到 GitHub releases（过滤 prerelease）
    try {
      final response = await http.get(Uri.parse(_githubApi));
      if (response.statusCode == 200) {
        final releases = json.decode(response.body) as List;
        final stableRelease = releases.cast<Map<String, dynamic>?>().firstWhere(
          (r) => r != null && r['prerelease'] == false,
          orElse: () => null,
        );
        if (stableRelease == null) return null;
        final version =
            (stableRelease['tag_name'] as String).replaceFirst('v', '');
        if (_compareVersions(version, currentVersion) > 0) {
          return UpdateInfo(
            version: version,
            url: stableRelease['html_url'] as String,
            body: stableRelease['body'] as String? ?? '',
          );
        }
      }
    } catch (_) {}
    return null;
  }

  /// 获取当前平台的 key（对应 latest.json files 中的 key）
  static Future<String> _getPlatformKey() async {
    if (Platform.isAndroid) {
      final deviceInfo = await DeviceInfoPlugin().androidInfo;
      final abi = deviceInfo.supportedAbis.first;
      if (abi.contains('arm64')) return 'android-arm64v8';
      if (abi.contains('x86_64')) return 'android-x64';
      return 'android-arm32v7';
    }
    if (Platform.isMacOS) return 'macos';
    if (Platform.isLinux) {
      final arch = await _getDesktopArch();
      if (arch.contains('aarch64') || arch.contains('arm64')) {
        return 'linux-arm-64-tar';
      }
      return 'linux-x86-64-tar';
    }
    if (Platform.isWindows) {
      final arch = await _getDesktopArch();
      if (arch.contains('arm64')) return 'windows-arm-64-zip';
      return 'windows-x86-64-exe';
    }
    return '';
  }

  /// 检测桌面端 CPU 架构
  static Future<String> _getDesktopArch() async {
    try {
      if (Platform.isWindows) {
        final result =
            await Process.run('cmd', ['/c', 'echo %PROCESSOR_ARCHITECTURE%']);
        return result.stdout.toString().trim();
      } else {
        final result = await Process.run('uname', ['-m']);
        return result.stdout.toString().trim();
      }
    } catch (_) {
      return 'x86_64';
    }
  }

  /// 比较版本号，返回正数表示 a > b
  static int _compareVersions(String a, String b) {
    final partsA = a.split('.').map((e) => int.tryParse(e) ?? 0).toList();
    final partsB = b.split('.').map((e) => int.tryParse(e) ?? 0).toList();
    final maxLen =
        partsA.length > partsB.length ? partsA.length : partsB.length;
    for (var i = 0; i < maxLen; i++) {
      final va = i < partsA.length ? partsA[i] : 0;
      final vb = i < partsB.length ? partsB[i] : 0;
      if (va != vb) return va - vb;
    }
    return 0;
  }
}