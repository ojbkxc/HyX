import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter_ble_peripheral/flutter_ble_peripheral.dart';
import 'package:flutter_blue_plus/flutter_blue_plus.dart';
import 'package:hyx_app/provider/device_provider.dart';
import 'package:hyx_isolates/rust/api/discovery.dart' as rust_discovery;
import 'package:logging/logging.dart';
import 'package:refena_flutter/refena_flutter.dart';

final _logger = Logger('BleSharing');

/// 已启动的 [RefenaContainer]，供非 widget 层 dispatch 设备状态。由 [attachRefena] 注入。
RefenaContainer? _refena;

/// 绑定 Refena 容器（在 `main.dart` 创建 container 后调用），
/// 使蓝牙发现到的候选 IP 能通过 [AddBluetoothCandidatesAction] 进入设备状态。
void attachRefena(RefenaContainer container) => _refena = container;

/// HyX BLE 服务：广告本机 IP + 扫描邻居 IP。
///
/// BLE 是"同局域网不同网段设备发现"的补充通道，**不参与在线判定**：
/// 广告端把本机局域网 IP 编码进一个保留的 [serviceUuid]；扫描端读到邻居的
/// serviceUuid 后解出 IP，投递给 [AddBluetoothCandidatesAction] 进候选池。
/// 候选 IP 能否互通、是否在线，仍由 Rust 层单播探测（[probe_peer]）决定，
/// 每次发现刷新都会重探测——蓝牙不改变"在线/离线"语义。
///
/// 只在移动端（Android / iOS）启用；桌面端为空实现。
class BleSharing {
  static final BleSharing instance = BleSharing._();

  /// HyX BLE 服务 UUID 固定前缀（16 字节）。后 8 位十六进制（4 字节）编码 IPv4。
  ///
  /// 完整 UUID = `785a5958-0000-0000-0000-0000` + `XXXXXXX`（IP 的 4 字节十六进制）。
  /// 手机间广告/扫描共用此前缀以互相识别。
  static const _uuidPrefix = '785a5958-0000-0000-0000-0000';

  bool _advertising = false;
  bool _scanning = false;

  /// 已上报告知的候选 IP，避免对同一 IP 重复投递。
  final Set<String> _reported = {};

  BleSharing._();

  /// 当前平台是否支持 BLE（仅移动端）。
  bool get supported =>
      !kIsWeb && (Platform.isAndroid || Platform.isIOS);

  /// 优雅启动：广告本机 IP + 开始扫描邻居。桌面端直接返回。
  Future<void> start() async {
    if (!supported) return;
    await advertise();
    await scan();
  }

  /// 停止广告与扫描。桌面端直接返回。
  Future<void> stop() async {
    if (!supported) return;
    await _stopAdvertise();
    await _stopScan();
  }

  /// 广告本机局域网 IP。
  Future<void> advertise() async {
    if (_advertising) return;
    String? ip;
    try {
      // 从 Rust 核心取本机合适的局域网 IPv4；失败则降级为仅广播前缀（无 IP）。
      ip = await rust_discovery.localWifiIp();
    } catch (e) {
      _logger.warning('localWifiIp failed: $e');
    }
    final uuid = _encodeUuid(ip);
    try {
      if (await FlutterBlePeripheral().isAdvertising) {
        await FlutterBlePeripheral().stop();
      }
      await FlutterBlePeripheral().start(
        advertiseData: AdvertiseData(
          serviceUuids: [uuid],
          localName: 'HyX',
        ),
      );
      _advertising = true;
      _logger.fine('BLE advertising uuid=$uuid');
    } catch (e) {
      _logger.warning('BLE advertise failed: $e');
    }
  }

  Future<void> _stopAdvertise() async {
    if (!_advertising) return;
    try {
      if (await FlutterBlePeripheral().isAdvertising) {
        await FlutterBlePeripheral().stop();
      }
    } catch (e) {
      _logger.warning('BLE stop advertise failed: $e');
    }
    _advertising = false;
  }

  /// 开始扫描邻居的 HyX BLE 服务，解析出候选 IP 进设备状态。
  Future<void> scan() async {
    if (_scanning || FlutterBluePlus.isScanningNow) return;
    try {
      FlutterBluePlus.scanResults.listen(_onScanResults);
      await FlutterBluePlus.startScan();
      _scanning = true;
    } catch (e) {
      _logger.warning('BLE scan start failed: $e');
    }
  }

  Future<void> _stopScan() async {
    if (!_scanning) return;
    try {
      await FlutterBluePlus.stopScan();
    } catch (e) {
      _logger.warning('BLE stop scan failed: $e');
    }
    _scanning = false;
  }

  /// 过滤扫描结果里的 HyX 服务 UUID，解出对端 IP 并投递到设备候选池。
  void _onScanResults(List<ScanResult> results) {
    final ips = <String>{};
    for (final r in results) {
      for (final guid in r.advertisementData.serviceUuids) {
        final ip = _decodeUuid(guid.toString());
        if (ip == null) continue;
        // _reported.add 返回 true 说明是新发现的候选 IP，才纳入本次投递。
        if (_reported.add(ip)) ips.add(ip);
      }
    }
    if (ips.isEmpty) return;
    _logger.fine('BLE scan found candidate IPs: $ips');
    final container = _refena;
    if (container != null) {
      container.redux(deviceProvider).dispatch(AddBluetoothCandidatesAction(ips));
    }
  }

  /// 把 IPv4 地址编码进 HyX 服务 UUID 的末尾 8 位十六进制。
  /// `ip` 非法或为空时返回仅含前缀的 UUID（扫描端会忽略，因为缺 IP 数据）。
  static String _encodeUuid(String? ip) {
    final hex = _ipToHex(ip);
    return hex == null ? _uuidPrefix : '$_uuidPrefix$hex';
  }

  /// 从尾部 8 位十六进制还原 IPv4；非 HyX 前缀或格式非法返回 null。
  static String? _decodeUuid(String uuid) {
    final upper = uuid.trim().toUpperCase();
    if (!upper.startsWith(_uuidPrefix)) return null;
    final hex = upper.substring(_uuidPrefix.length);
    if (hex.length != 8) return null;
    final bytes = <int>[];
    for (var i = 0; i < hex.length; i += 2) {
      final v = int.tryParse(hex.substring(i, i + 2), radix: 16);
      if (v == null) return null;
      bytes.add(v);
    }
    return bytes.join('.');
  }

  /// IPv4 → 8 位小写十六进制（4 字节）。非法返回 null。
  static String? _ipToHex(String? ip) {
    if (ip == null) return null;
    final parts = ip.split('.');
    if (parts.length != 4) return null;
    final sb = StringBuffer();
    for (final p in parts) {
      final v = int.tryParse(p);
      if (v == null || v < 0 || v > 255) return null;
      sb.write(v.toRadixString(16).padLeft(2, '0'));
    }
    return sb.toString();
  }
}