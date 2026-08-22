import 'dart:async';
import 'dart:convert';

import 'package:hyx_isolates/rust/api/device.dart' as rust_device;
import 'package:hyx_isolates/rust/api/discovery.dart' as rust_discovery;
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:logging/logging.dart';
import 'package:refena_flutter/refena_flutter.dart';
import 'package:shared_preferences/shared_preferences.dart';

final _logger = Logger('DeviceProvider');

/// 已知设备持久化在 SharedPreferences 的 key。
const _kKnownDevicesPrefKey = 'hyx_known_devices';

/// 已知设备（含接收/禁止状态与最后已知地址）。
///
/// 用 [deviceId]（Uuid 字符串）作为唯一标识，跨在线/历史状态复用。
/// 当设备从局域网消失时仍保留在历史列表中；重新上线时复用其 [allowReceive] 状态。
class KnownDevice {
  /// 设备唯一标识（Uuid 字符串）。
  final String deviceId;

  /// 设备显示名。
  final String name;

  /// 最后已知地址（ip:port）。
  final String addr;

  /// 是否允许接收来自此设备的文件传输。默认 true。
  final bool allowReceive;

  /// 最后一次在线时间（epoch 毫秒）。
  final int lastSeen;

  const KnownDevice({
    required this.deviceId,
    required this.name,
    required this.addr,
    this.allowReceive = true,
    required this.lastSeen,
  });

  KnownDevice copyWith({
    String? name,
    String? addr,
    bool? allowReceive,
    int? lastSeen,
  }) =>
      KnownDevice(
        deviceId: deviceId,
        name: name ?? this.name,
        addr: addr ?? this.addr,
        allowReceive: allowReceive ?? this.allowReceive,
        lastSeen: lastSeen ?? this.lastSeen,
      );

  Map<String, dynamic> toJson() => {
        'deviceId': deviceId,
        'name': name,
        'addr': addr,
        'allowReceive': allowReceive,
        'lastSeen': lastSeen,
      };

  static KnownDevice fromJson(Map<String, dynamic> json) => KnownDevice(
        deviceId: json['deviceId'] as String,
        name: json['name'] as String,
        addr: json['addr'] as String,
        allowReceive: (json['allowReceive'] as bool?) ?? true,
        lastSeen: (json['lastSeen'] as num).toInt(),
      );
}

/// 设备发现状态。
class DeviceState {
  /// 是否正在扫描。
  final bool scanning;

  /// 当前在线的 peer 列表（局域网发现结果，按 deviceId 去重）。
  final List<model.RsDiscoveredPeer> peers;

  /// 已知设备表（含历史设备），以 deviceId 字符串为 key。
  final Map<String, KnownDevice> knownDevices;

  /// 本设备信息。`null` 表示尚未加载。
  final model.RsDevice? myDevice;

  /// 自动发现是否启用（定时 5s 刷新）。
  final bool autoDiscovery;

  /// 是否已从持久化加载过已知设备。
  final bool knownLoaded;

  const DeviceState({
    this.scanning = false,
    this.peers = const [],
    this.knownDevices = const {},
    this.myDevice,
    this.autoDiscovery = true,
    this.knownLoaded = false,
  });

  DeviceState copyWith({
    bool? scanning,
    List<model.RsDiscoveredPeer>? peers,
    Map<String, KnownDevice>? knownDevices,
    model.RsDevice? myDevice,
    bool? autoDiscovery,
    bool? knownLoaded,
  }) =>
      DeviceState(
        scanning: scanning ?? this.scanning,
        peers: peers ?? this.peers,
        knownDevices: knownDevices ?? this.knownDevices,
        myDevice: myDevice ?? this.myDevice,
        autoDiscovery: autoDiscovery ?? this.autoDiscovery,
        knownLoaded: knownLoaded ?? this.knownLoaded,
      );

  /// 历史设备列表：knownDevices 中不在当前 peers 里的设备，按 lastSeen 倒序。
  List<KnownDevice> get historyDevices {
    final onlineIds = <String>{};
    for (final p in peers) {
      onlineIds.add(p.deviceId.toString());
    }
    final hist = knownDevices.values.where((d) => !onlineIds.contains(d.deviceId)).toList();
    hist.sort((a, b) => b.lastSeen.compareTo(a.lastSeen));
    return hist;
  }

  /// 在线设备的 KnownDevice 视图（合并持久化的 allowReceive 状态），按 peers 顺序。
  /// 若某在线设备尚未在 knownDevices 中（理论上 RefreshPeersAction 已自动加入），
  /// 则用默认值（allowReceive=true）构造一个临时实例。
  List<KnownDevice> get onlineDevices {
    final result = <KnownDevice>[];
    for (final p in peers) {
      final id = p.deviceId.toString();
      final known = knownDevices[id];
      if (known != null) {
        // 同步最新的 name/addr（持久化值可能过期）。
        result.add(known.copyWith(name: p.name, addr: p.addr));
      } else {
        result.add(KnownDevice(
          deviceId: id,
          name: p.name,
          addr: p.addr,
          allowReceive: true,
          lastSeen: DateTime.now().millisecondsSinceEpoch,
        ));
      }
    }
    return result;
  }

  /// 查询某个设备的 allowReceive 状态。未知设备默认 true（自动接收）。
  bool isAllowed(String deviceId) {
    final known = knownDevices[deviceId];
    return known?.allowReceive ?? true;
  }

  static const initial = DeviceState();
}

/// 设备发现状态管理。
///
/// 使用 [ReduxProvider]：`StartDiscoveryAction` 启动定时刷新循环，
/// `RefreshPeersAction` 执行一次 `discover` 并同步更新 [KnownDevice] 表，
/// `StopDiscoveryAction` 停止循环。`LoadKnownDevicesAction` 从 SharedPreferences
/// 加载持久化的已知设备列表（含接收/禁止状态）。
final deviceProvider = ReduxProvider<DeviceService, DeviceState>((ref) => DeviceService());

class DeviceService extends ReduxNotifier<DeviceState> {
  @override
  DeviceState init() => DeviceState.initial;

  /// 定时刷新计时器。
  Timer? _timer;

  /// 把已知设备表持久化到 SharedPreferences（JSON 编码）。
  Future<void> _persistKnown(Map<String, KnownDevice> devices) async {
    try {
      final prefs = await SharedPreferences.getInstance();
      final raw = jsonEncode(devices.values.map((d) => d.toJson()).toList());
      await prefs.setString(_kKnownDevicesPrefKey, raw);
    } catch (e) {
      _logger.warning('persist known devices failed: $e');
    }
  }
}

/// 从 SharedPreferences 异步加载已知设备列表。由 `preInit` 触发一次。
class LoadKnownDevicesAction extends AsyncReduxAction<DeviceService, DeviceState> {
  @override
  Future<DeviceState> reduce() async {
    if (state.knownLoaded) return state;
    try {
      final prefs = await SharedPreferences.getInstance();
      final raw = prefs.getString(_kKnownDevicesPrefKey);
      if (raw == null) return state.copyWith(knownLoaded: true);
      final list = jsonDecode(raw) as List<dynamic>;
      final map = <String, KnownDevice>{};
      for (final e in list) {
        final d = KnownDevice.fromJson(e as Map<String, dynamic>);
        map[d.deviceId] = d;
      }
      return state.copyWith(knownDevices: map, knownLoaded: true);
    } catch (e) {
      _logger.warning('load known devices failed: $e');
      return state.copyWith(knownLoaded: true);
    }
  }
}

/// 加载本设备身份。对应 Rust `create_device`。
class LoadMyDeviceAction extends AsyncReduxAction<DeviceService, DeviceState> {
  @override
  Future<DeviceState> reduce() async {
    if (state.myDevice != null) return state;
    try {
      final dev = await rust_device.createDevice();
      return state.copyWith(myDevice: dev);
    } catch (e) {
      _logger.warning('createDevice failed: $e');
      return state;
    }
  }
}

/// 启动自动发现：立即刷新一次，然后每 5 秒刷新。
class StartDiscoveryAction extends ReduxAction<DeviceService, DeviceState> {
  @override
  DeviceState reduce() {
    if (!state.autoDiscovery) return state;
    notifier._timer?.cancel();
    // 立即触发一次刷新。
    unawaited(dispatchAsync(RefreshPeersAction()));
    notifier._timer = Timer.periodic(const Duration(seconds: 5), (_) {
      unawaited(dispatchAsync(RefreshPeersAction()));
    });
    return state.copyWith(scanning: true);
  }
}

/// 停止自动发现。
class StopDiscoveryAction extends ReduxAction<DeviceService, DeviceState> {
  @override
  DeviceState reduce() {
    notifier._timer?.cancel();
    notifier._timer = null;
    return state.copyWith(scanning: false, autoDiscovery: false);
  }
}

/// 手动刷新一次 peer 列表。对应 Rust `discover`。
///
/// 同时把在线设备合并到 [DeviceState.knownDevices]：新设备自动加入（默认
/// allowReceive=true），已存在设备更新 name/addr/lastSeen。这样设备从在线变为
/// 离线时仍保留在历史列表中，重新上线时复用其 allowReceive 状态。
class RefreshPeersAction extends AsyncReduxAction<DeviceService, DeviceState> {
  /// 发现端口，0 表示使用默认 14567。
  final int port;

  RefreshPeersAction({this.port = 0});

  @override
  Future<DeviceState> reduce() async {
    try {
      final peers = await rust_discovery.discover(port: port);
      // 按 deviceId 去重，保留最后发现的地址。
      final map = <String, model.RsDiscoveredPeer>{};
      for (final p in peers) {
        map[p.deviceId.toString()] = p;
      }
      final dedupPeers = map.values.toList();

      // 合并到 knownDevices：更新在线设备的 name/addr/lastSeen，新设备自动加入。
      // lastSeen 每 60 秒最多更新一次，避免 5s 定时刷新导致频繁持久化写入。
      final now = DateTime.now().millisecondsSinceEpoch;
      final known = Map<String, KnownDevice>.from(state.knownDevices);
      var changed = false;
      for (final p in dedupPeers) {
        final id = p.deviceId.toString();
        final existing = known[id];
        if (existing == null) {
          known[id] = KnownDevice(
            deviceId: id,
            name: p.name,
            addr: p.addr,
            allowReceive: true,
            lastSeen: now,
          );
          changed = true;
        } else {
          final stale = now - existing.lastSeen > 60000;
          if (existing.name != p.name || existing.addr != p.addr || stale) {
            known[id] = existing.copyWith(name: p.name, addr: p.addr, lastSeen: now);
            changed = true;
          }
        }
      }

      if (changed) {
        unawaited(notifier._persistKnown(known));
      }

      return state.copyWith(peers: dedupPeers, knownDevices: known);
    } catch (e) {
      _logger.warning('discover failed: $e');
      return state;
    }
  }
}

/// 切换设备的接收/禁止状态。设备必须在 [DeviceState.knownDevices] 中。
///
/// 注意：当前 Rust 侧 `start_listener` 无条件接受所有连接（无法获取发送方
/// device_id），因此此状态目前仅作持久化记录，未来 Rust 侧支持按 device_id
/// 过滤后即可生效。
class ToggleAllowReceiveAction extends AsyncReduxAction<DeviceService, DeviceState> {
  final String deviceId;

  ToggleAllowReceiveAction(this.deviceId);

  @override
  Future<DeviceState> reduce() async {
    final existing = state.knownDevices[deviceId];
    if (existing == null) return state;
    final known = Map<String, KnownDevice>.from(state.knownDevices);
    known[deviceId] = existing.copyWith(allowReceive: !existing.allowReceive);
    unawaited(notifier._persistKnown(known));
    return state.copyWith(knownDevices: known);
  }
}

/// 删除已知设备（历史设备）。若设备当前在线则不删除（UI 层应保证不触发）。
class RemoveKnownDeviceAction extends AsyncReduxAction<DeviceService, DeviceState> {
  final String deviceId;

  RemoveKnownDeviceAction(this.deviceId);

  @override
  Future<DeviceState> reduce() async {
    // 不允许删除当前在线的设备。
    final isOnline = state.peers.any((p) => p.deviceId.toString() == deviceId);
    if (isOnline) return state;
    if (!state.knownDevices.containsKey(deviceId)) return state;
    final known = Map<String, KnownDevice>.from(state.knownDevices);
    known.remove(deviceId);
    unawaited(notifier._persistKnown(known));
    return state.copyWith(knownDevices: known);
  }
}

/// 切换自动发现开关。
class ToggleAutoDiscoveryAction extends ReduxAction<DeviceService, DeviceState> {
  final bool enabled;

  ToggleAutoDiscoveryAction(this.enabled);

  @override
  DeviceState reduce() {
    if (enabled) {
      dispatch(StartDiscoveryAction());
      return state.copyWith(autoDiscovery: true);
    } else {
      dispatch(StopDiscoveryAction());
      return state.copyWith(autoDiscovery: false);
    }
  }
}
