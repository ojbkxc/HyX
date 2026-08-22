import 'dart:async';

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
      );

  static const idle = TransferState();
}

/// 传输状态管理。
///
/// 使用 [ReduxProvider]：所有 mutate 都通过 dispatch action，便于追踪 + 测试。
/// Rust 侧 `start_listener` / `connect` / `pair_*` 通过 `StreamSink<RsProgressEvent>`
/// 推送进度，action 内 `sink` 转 `Stream.listen` → dispatch [_UpdateProgressAction]。
final transferProvider = ReduxProvider<TransferService, TransferState>((ref) => TransferService());

class TransferService extends ReduxNotifier<TransferState> {
  @override
  TransferState init() => TransferState.idle;

  /// 当前活动的进度流订阅，cancel / 完成后取消。
  StreamSubscription? _sub;
}

/// 启动监听接收。
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
    notifier._sub?.cancel();

    final dir = saveDir ?? (await _defaultSaveDir());

    try {
      final stream = rust_transfer.startListener(port: port, chunkBytes: chunkBytes, compression: compression, saveDir: dir);
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      notifier._sub?.cancel();
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

/// 直连发送文件到 `peerAddress`。
///
/// 对应 Rust `connect`。`peerAddress` 为空时由 Rust 侧自动发现 LAN peer。
class StartSendAction extends AsyncReduxAction<TransferService, TransferState> {
  final String peerAddress;
  final String filePath;
  final int port;
  final int chunkBytes;
  final int compression;

  StartSendAction({
    required this.peerAddress,
    required this.filePath,
    this.port = 0,
    this.chunkBytes = 1024 * 1024,
    this.compression = 1,
  });

  @override
  Future<TransferState> reduce() async {
    if (state.busy) return state;
    notifier._sub?.cancel();

    final name = filePath.split(RegExp(r'[/\\]')).last;
    try {
      final stream = rust_transfer.connect(peerAddress: peerAddress, filePath: filePath, chunkBytes: chunkBytes, compression: compression, port: port);
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      notifier._sub?.cancel();
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

/// 经 rendezvous 服务器配对，作为接收方。
class StartPairReceiveAction extends AsyncReduxAction<TransferService, TransferState> {
  final String code;
  final String server;
  final int port;
  final int compression;
  final String? saveDir;

  StartPairReceiveAction({
    required this.code,
    required this.server,
    this.port = 0,
    this.compression = 1,
    this.saveDir,
  });

  @override
  Future<TransferState> reduce() async {
    if (state.busy) return state;
    notifier._sub?.cancel();

    final dir = saveDir ?? (await _defaultSaveDir());

    try {
      final stream = rust_transfer.pairRendezvous(code: code, server: server, port: port, compression: compression, saveDir: dir);
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      notifier._sub?.cancel();
      notifier._sub = null;
      return state.copyWith(
        status: model.RsTransferStatus.failed,
        errorMessage: e.toString(),
        endTime: DateTime.now(),
      );
    }
    return state.copyWith(
      direction: RsTransferDirection.receive,
      status: model.RsTransferStatus.pairing,
      peerAddress: server,
      startTime: DateTime.now(),
      clearError: true,
    );
  }
}

/// 经 rendezvous 服务器配对，作为发送方。
class StartPairSendAction extends AsyncReduxAction<TransferService, TransferState> {
  final String code;
  final String server;
  final String filePath;
  final int port;
  final int chunkBytes;
  final int compression;

  StartPairSendAction({
    required this.code,
    required this.server,
    required this.filePath,
    this.port = 0,
    this.chunkBytes = 1024 * 1024,
    this.compression = 1,
  });

  @override
  Future<TransferState> reduce() async {
    if (state.busy) return state;
    notifier._sub?.cancel();

    final name = filePath.split(RegExp(r'[/\\]')).last;
    try {
      final stream = rust_transfer.pairSend(code: code, server: server, port: port, filePath: filePath, chunkBytes: chunkBytes, compression: compression);
      notifier._sub = stream.listen((e) => dispatch(_UpdateProgressAction(e)));
    } catch (e) {
      notifier._sub?.cancel();
      notifier._sub = null;
      return state.copyWith(
        status: model.RsTransferStatus.failed,
        errorMessage: e.toString(),
        endTime: DateTime.now(),
      );
    }
    return state.copyWith(
      direction: RsTransferDirection.send,
      status: model.RsTransferStatus.pairing,
      fileName: name,
      peerAddress: server,
      startTime: DateTime.now(),
      clearError: true,
    );
  }
}

/// 取消当前传输。对应 Rust `cancel`。
class CancelTransferAction extends ReduxAction<TransferService, TransferState> {
  @override
  TransferState reduce() {
    if (!state.busy) return state;
    try {
      rust_transfer.cancel();
    } catch (e) {
      _logger.warning('cancel failed: $e');
    }
    notifier._sub?.cancel();
    notifier._sub = null;
    return state.copyWith(
      status: model.RsTransferStatus.cancelled,
      endTime: DateTime.now(),
    );
  }
}

/// 重置回 idle（终态后由 UI 调用）。
class ResetTransferAction extends ReduxAction<TransferService, TransferState> {
  @override
  TransferState reduce() {
    notifier._sub?.cancel();
    notifier._sub = null;
    return TransferState.idle;
  }
}

/// 内部：处理 RsProgressEvent。
class _UpdateProgressAction extends ReduxAction<TransferService, TransferState> {
  final model.RsProgressEvent event;

  _UpdateProgressAction(this.event);

  @override
  TransferState reduce() {
    final isDone = event.status == model.RsTransferStatus.completed ||
        event.status == model.RsTransferStatus.failed ||
        event.status == model.RsTransferStatus.cancelled;
    if (isDone) {
      notifier._sub?.cancel();
      notifier._sub = null;
    }
    return state.copyWith(
      status: event.status,
      transferred: event.transferred.toInt(),
      total: event.total.toInt(),
      speed: event.speed,
      endTime: isDone ? DateTime.now() : null,
      errorMessage: event.message,
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