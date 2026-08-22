import 'dart:async';

import 'package:hyx_isolates/rust/api/device.dart' as rust_device;
import 'package:hyx_isolates/rust/api/discovery.dart' as rust_discovery;
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:logging/logging.dart';
import 'package:refena_flutter/refena_flutter.dart';

final _logger = Logger('DeviceProvider');

/// 设备发现状态。
class DeviceState {
  /// 是否正在扫描。
  final bool scanning;

  /// 已发现的 peer 列表。
  final List<model.RsDiscoveredPeer> peers;

  /// 本设备信息。`null` 表示尚未加载。
  final model.RsDevice? myDevice;

  /// 自动发现是否启用（定时 5s 刷新）。
  final bool autoDiscovery;

  const DeviceState({
    this.scanning = false,
    this.peers = const [],
    this.myDevice,
    this.autoDiscovery = true,
  });

  DeviceState copyWith({
    bool? scanning,
    List<model.RsDiscoveredPeer>? peers,
    model.RsDevice? myDevice,
    bool? autoDiscovery,
  }) =>
      DeviceState(
        scanning: scanning ?? this.scanning,
        peers: peers ?? this.peers,
        myDevice: myDevice ?? this.myDevice,
        autoDiscovery: autoDiscovery ?? this.autoDiscovery,
      );

  static const initial = DeviceState();
}

/// 设备发现状态管理。
///
/// 使用 [ReduxProvider]：`StartDiscoveryAction` 启动定时刷新循环，
/// `RefreshPeersAction` 执行一次 `discover`，`StopDiscoveryAction` 停止循环。
/// `LoadMyDeviceAction` 加载本设备身份。
final deviceProvider = ReduxProvider<DeviceService, DeviceState>((ref) => DeviceService());

class DeviceService extends ReduxNotifier<DeviceState> {
  @override
  DeviceState init() => DeviceState.initial;

  /// 定时刷新计时器。
  Timer? _timer;
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
class RefreshPeersAction extends AsyncReduxAction<DeviceService, DeviceState> {
  /// 发现端口，0 表示使用默认 14567。
  final int port;

  RefreshPeersAction({this.port = 0});

  @override
  Future<DeviceState> reduce() async {
    try {
      final peers = await rust_discovery.discover(port);
      // 按 deviceId 去重，保留最后发现的地址。
      final map = <String, model.RsDiscoveredPeer>{};
      for (final p in peers) {
        map[p.deviceId.toString()] = p;
      }
      return state.copyWith(peers: map.values.toList());
    } catch (e) {
      _logger.warning('discover failed: $e');
      return state;
    }
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