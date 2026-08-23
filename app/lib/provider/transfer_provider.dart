import 'dart:async';

import 'package:hyx_app/provider/device_provider.dart';
import 'package:hyx_app/util/transfer_direction.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:hyx_isolates/rust/api/transfer.dart' as rust_transfer;
import 'package:logging/logging.dart';
import 'package:path_provider/path_provider.dart';
import 'package:refena_flutter/refena_flutter.dart';

final _logger = Logger('TransferProvider');

/// 传输状态视图模型。
///
/// 对应 Kotlin `TransferProgress`：把 Rust 侧 `RsProgressEvent` 流聚合成单条
/// 可被 UI 直接观察的状态。`startTime` 用于计算耗时；`fileName` / `peerAddress`
/// 供历史记录使用。
///
/// [autoListening] 表示当前是否处于自动监听模式（应用启动后持续接收），
/// 传输完成后会自动重启监听。
class TransferState {
  final RsTransferDirection direction;
  final model.RsTransferStatus status;
  final int transferred;
  final int total;
  final double speed; // 字节/秒
  final String fileName;
  final String peerAddress;
  final DateTime? startTime;
  final DateTime? endTime;
  final String? errorMessage;
  final bool autoListening;

  /// TOFU 连接成功后回传的对端指纹（hex），由 `_UpdateProgressAction` 从
  /// `RsProgressEvent.peerFingerprint` 写入。非 null 表示本次传输走了 TOFU 路径，
  /// 已通过 `UpdateDeviceFingerprintByAddrAction` 缓存到 `KnownDevice.fingerprint`。
  /// 供 UI / 调试观察；传输重置后随 state 一起清空。
  final String? peerFingerprint;

  const TransferState({
    this.direction = RsTransferDirection.send,
    this.status = model.RsTransferStatus.idle,
    this.transferred = 0,
    this.total = 0,
    this.speed = 0,
    this.fileName = '',
    this.peerAddress = '',
    this.startTime,
    this.endTime,
    this.errorMessage,
    this.autoListening = false,
    this.peerFingerprint,
  });

  /// 进度分数 0..1。`total == 0` 时返回 0。
  double get fraction => total > 0 ? (transferred / total).clamp(0.0, 1.0) : 0.0;

  /// 是否处于忙态（pairing/connecting/transferring）。
  bool get busy =>
      status == model.RsTransferStatus.pairing ||
      status == model.RsTransferStatus.connecting ||
      status == model.RsTransferStatus.transferring;

  /// 是否终态（completed/failed/cancelled）。
  bool get done =>
      status == model.RsTransferStatus.completed ||
      status == model.RsTransferStatus.failed ||
      status == model.RsTransferStatus.cancelled;

  TransferState copyWith({
    RsTransferDirection? direction,
    model.RsTransferStatus? status,
    int? transferred,
    int? total,
    double? speed,
    String? fileName,
    String? peerAddress,
    DateTime? startTime,
    DateTime? endTime,
    String? errorMessage,
    bool clearError = false,
    bool? autoListening,
    String? peerFingerprint,
  }) =>
      TransferState(
        direction: direction ?? this.direction,
        status: status ?? this.status,
        transferred: transferred ?? this.transferred,
        total: total ?? this.total,
        speed: speed ?? this.speed,
        fileName: fileName ?? this.fileName,
        peerAddress: peerAddress ?? this.peerAddress,
        startTime: startTime ?? this.startTime,
        endTime: endTime ?? this.endTime,
        errorMessage: clearError ? null : (errorMessage ?? this.errorMessage),
        autoListening: autoListening ?? this.autoListening,
        peerFingerprint: peerFingerprint ?? this.peerFingerprint,
      );

  static const idle = TransferState();
}

/// 传输状态管理。
///
/// 使用 [ReduxProvider]：所有 mutate 都通过 dispatch action，便于追踪 + 测试。
/// Rust 侧 `start_listener` / `connect` / `pair_*` 通过 `StreamSink<RsProgressEvent>`
/// 推送进度，action 内 `sink` 转 `Stream.listen` → dispatch [_UpdateProgressAction]。
///
/// [StartAutoListenAction] 用于应用启动时自动监听 incoming 连接（不需要用户手动
/// 点 FAB）。传输完成后自动重启监听，实现持续接收。
///
/// 通过构造函数注入 [DeviceService]（Refena 推荐的依赖注入方式），使 action 能
/// 读取 [DeviceState.blockedDeviceIds] 并同步到 Rust 侧。
final transferProvider = ReduxProvider<TransferService, TransferState>((ref) {
  return TransferService(ref.notifier(deviceProvider));
});

class TransferService extends ReduxNotifier<TransferState> {
  /// 设备状态 notifier，用于在 action 内读取被禁止的设备 ID 列表。
  final DeviceService _deviceService;

  TransferService(this._deviceService);

  @override
  TransferState init() => TransferState.idle;

  /// 当前活动的进度流订阅，cancel / 完成后取消。
  StreamSubscription<model.RsProgressEvent>? _sub;

  /// 把 [DeviceProvider] 中 `allowReceive == false` 的设备 ID 列表同步到 Rust 侧。
  ///
  /// 在每次启动监听前调用，使 Rust `receive_into` 能按发送方设备 ID 拒收被禁止的设备。
  /// fire-and-forget：失败仅记日志，不阻塞监听启动（最坏情况是 Rust 侧沿用上一次列表）。
  void _syncBlockedDevices() {
    try {
      final blockedIds = _deviceService.state.blockedDeviceIds;
      unawaited(rust_transfer.setBlockedDevices(ids: blockedIds));
    } catch (e) {
      _logger.warning('setBlockedDevices failed: $e');
    }
  }
}

/// 启动监听接收（手动模式，UI 显示"连接中"状态）。
///
/// 对应 Rust `start_listener`：绑定 `port` + 接收对端文件到 `saveDir`。
/// `saveDir` 为空时使用 `getApplicationDocumentsDirectory()/hyx_received`。
class StartReceiveAction extends AsyncReduxAction<TransferService, TransferState> {
  final int port;
  final int chunkBytes;
  final int compression;
  final String? saveDir;

  StartReceiveAction({
    this.port = 0,
    this.chunkBytes = 1024 * 1024,
    this.compression = 1,
    this.saveDir,
  });

  @override
  Future<TransferState> reduce() async {
    if (state.busy) return state;
    unawaited(notifier._sub?.cancel());

    // 启动监听前把当前禁止列表同步到 Rust，使 receive_into 能按设备 ID 拒收。
    notifier._syncBlockedDevices();

    final dir = saveDir ?? (await _defaultSaveDir());

    try {
      final stream = rust_transfer.startListener(port: port, chunkBytes: chunkBytes, compression: compression, saveDir: dir);
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      unawaited(notifier._sub?.cancel());
      notifier._sub = null;
      return state.copyWith(
        status: model.RsTransferStatus.failed,
        errorMessage: e.toString(),
        endTime: DateTime.now(),
      );
    }
    return state.copyWith(
      direction: RsTransferDirection.receive,
      status: model.RsTransferStatus.connecting,
      startTime: DateTime.now(),
      clearError: true,

    );
  }
}

/// 启动自动监听（应用启动时调用，不显示"连接中"状态）。
///
/// 与 [StartReceiveAction] 的区别：
/// - 不把状态设为 `connecting`，保持 `idle`，避免 UI 误判为忙态。
/// - 设 `autoListening=true`，传输完成后自动重启监听（持续接收）。
/// - 若已有活动订阅则不重复启动。
///
/// 这样手机随时可以发送文件到电脑，无需用户手动点 FAB。
class StartAutoListenAction extends AsyncReduxAction<TransferService, TransferState> {
  final int port;
  final int chunkBytes;
  final int compression;
  final String? saveDir;

  StartAutoListenAction({
    this.port = 0,
    this.chunkBytes = 1024 * 1024,
    this.compression = 1,
    this.saveDir,
  });

  @override
  Future<TransferState> reduce() async {
    // 已有活动订阅则不重复启动。
    if (notifier._sub != null) return state;
    unawaited(notifier._sub?.cancel());

    // 启动监听前把当前禁止列表同步到 Rust，使 receive_into 能按设备 ID 拒收。
    notifier._syncBlockedDevices();

    final dir = saveDir ?? (await _defaultSaveDir());

    try {
      final stream = rust_transfer.startListener(port: port, chunkBytes: chunkBytes, compression: compression, saveDir: dir);
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      unawaited(notifier._sub?.cancel());
      notifier._sub = null;
      _logger.warning('auto listen failed: $e');
      // 出错时不改变 UI 状态，仅记录日志。后续可重试。
      return state.copyWith(autoListening: true);
    }
    return state.copyWith(
      autoListening: true,
      clearError: true,
    );
  }
}

/// 停止自动监听。取消订阅并清除 autoListening 标志。
class StopAutoListenAction extends ReduxAction<TransferService, TransferState> {
  @override
  TransferState reduce() {
    unawaited(notifier._sub?.cancel());
    notifier._sub = null;
    return state.copyWith(autoListening: false);
  }
}

/// 直连发送文件到 `peerAddress`。
///
/// 对应 Rust `connect`：统一发送入口，内部按 `cachedFingerprint` 决策：
/// - 有缓存指纹 → 直接 pin 连接，跳过 UDP 发现；
/// - 无缓存但有地址 → 短超时发现拿指纹 → pin 连接，发现失败回退 TOFU；
/// - 无地址 → 自动发现（原行为）。
///
/// TOFU 连接成功后，Rust 侧通过 `RsProgressEvent.peerFingerprint` 回传实际指纹，
/// 由 [_UpdateProgressAction] 缓存到 `KnownDevice.fingerprint`，后续连接直接 pin。
class StartSendAction extends AsyncReduxAction<TransferService, TransferState> {
  final String peerAddress;
  /// 缓存的对端指纹（hex），从 `KnownDevice.fingerprint` 传入。
  /// null / 空串视为无缓存，走发现 / TOFU 回退路径。
  final String? cachedFingerprint;
  final String filePath;
  final int port;
  final int chunkBytes;
  final int compression;

  StartSendAction({
    required this.peerAddress,
    this.cachedFingerprint,
    required this.filePath,
    this.port = 0,
    this.chunkBytes = 1024 * 1024,
    this.compression = 1,
  });

  @override
  Future<TransferState> reduce() async {
    if (state.busy) return state;
    unawaited(notifier._sub?.cancel());

    final name = filePath.split(RegExp(r'[/\\]')).last;
    try {
      final stream = rust_transfer.connect(
        peerAddress: peerAddress,
        filePath: filePath,
        chunkBytes: chunkBytes,
        compression: compression,
        port: port,
        cachedFingerprint: cachedFingerprint,
      );
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      unawaited(notifier._sub?.cancel());
      notifier._sub = null;
      return state.copyWith(
        status: model.RsTransferStatus.failed,
        errorMessage: e.toString(),
        endTime: DateTime.now(),
      );
    }
    return state.copyWith(
      direction: RsTransferDirection.send,
      status: model.RsTransferStatus.connecting,
      fileName: name,
      peerAddress: peerAddress,
      startTime: DateTime.now(),
      clearError: true,
    );
  }
}


/// 取消当前传输。对应 Rust `cancel`。
///
/// 自动监听模式下，取消后重启监听以持续接收下一次传输。
class CancelTransferAction extends ReduxAction<TransferService, TransferState> {
  @override
  TransferState reduce() {
    if (!state.busy) return state;
    try {
      unawaited(rust_transfer.cancel());
    } catch (e) {
      _logger.warning('cancel failed: $e');
    }
    unawaited(notifier._sub?.cancel());
    notifier._sub = null;
    // 自动监听模式下，取消后重启监听。
    if (state.autoListening) {
      unawaited(dispatchAsync(StartAutoListenAction()));
    }
    return state.copyWith(
      status: model.RsTransferStatus.cancelled,
      endTime: DateTime.now(),
    );
  }
}

/// 重置回 idle（终态后由 UI 调用）。
///
/// 若处于自动监听模式，[_UpdateProgressAction] 已在终态时重启了监听订阅，
/// 此处不再 cancel/restart，仅重置 UI 状态。非自动监听模式下才 cancel 订阅。
class ResetTransferAction extends ReduxAction<TransferService, TransferState> {
  @override
  TransferState reduce() {
    final wasAutoListening = state.autoListening;
    if (!wasAutoListening) {
      // 非自动监听模式：取消订阅。
      unawaited(notifier._sub?.cancel());
      notifier._sub = null;
    }
    // 自动监听模式：保留 _sub（_UpdateProgressAction 已重启监听）。
    return TransferState.idle.copyWith(autoListening: wasAutoListening);
  }
}

/// 内部：处理 RsProgressEvent。
///
/// 当事件为终态（completed/failed/cancelled）时取消订阅。若处于自动监听模式，
/// 终态后自动重启监听以持续接收下一次传输。
class _UpdateProgressAction extends ReduxAction<TransferService, TransferState> {
  final model.RsProgressEvent event;

  _UpdateProgressAction(this.event);

  @override
  TransferState reduce() {
    final isDone = event.status == model.RsTransferStatus.completed ||
        event.status == model.RsTransferStatus.failed ||
        event.status == model.RsTransferStatus.cancelled;
    if (isDone) {
      unawaited(notifier._sub?.cancel());
      notifier._sub = null;
      // 自动监听模式下，传输完成后重启监听。
      if (state.autoListening) {
        unawaited(dispatchAsync(StartAutoListenAction()));
      }
    }

    // TOFU 连接成功后，Rust 侧通过 peerFingerprint 回传对端实际指纹（hex）。
    // 非 null 且非空 → 本次走了 TOFU 路径，把指纹缓存到匹配的 KnownDevice，
    // 后续连接直接 pin 跳过 UDP 发现。通过 peerAddress 关联已知设备。
    if (event.peerFingerprint != null && event.peerFingerprint!.isNotEmpty) {
      // 用 external() 包裹跨 notifier 调用，避免 invalid_use_of_internal_member0
      // （refena_flutter 的 dispatchAsync 是 @internal，external() 解除该限制）
      unawaited(external(notifier._deviceService).dispatchAsync(
        UpdateDeviceFingerprintByAddrAction(state.peerAddress, event.peerFingerprint!),
      ));
    }

    // 自动监听接收时 startTime 未在 action 中设置，首次收到 transferring 事件时补上，
    // 确保 [TransferProgressSheet] 能正确计算耗时与添加历史记录。
    final shouldSetStart = state.startTime == null &&
        event.status == model.RsTransferStatus.transferring;
    // 同步传输方向（event.direction 是 FRB 类型，需映射到 Dart 侧枚举）。
    final dir = event.direction == model.RsTransferDirection.send
        ? RsTransferDirection.send
        : RsTransferDirection.receive;
    return state.copyWith(
      direction: dir,
      status: event.status,
      transferred: event.transferred.toInt(),
      total: event.total.toInt(),
      speed: event.speed,
      startTime: shouldSetStart ? DateTime.now() : null,
      endTime: isDone ? DateTime.now() : null,
      errorMessage: event.message,
      peerFingerprint: event.peerFingerprint,
    );
  }
}

Future<String> _defaultSaveDir() async {
  try {
    final base = await getApplicationDocumentsDirectory();
    final dir = '${base.path}/hyx_received';
    return dir;
  } catch (e) {
    _logger.warning('getApplicationDocumentsDirectory failed: $e');
    return './hyx_received';
  }
}
