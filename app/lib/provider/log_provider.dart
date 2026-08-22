import 'dart:async';


import 'package:hyx_isolates/rust/api/logging.dart' as rust_logging;
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:logging/logging.dart';
import 'package:refena_flutter/refena_flutter.dart';

final _logger = Logger('LogProvider');

/// 单条日志条目。封装 `RsLogEvent` + 接收时间戳。
class LogEntry {
  final model.RsLogLevel level;
  final String tag;
  final String message;
  final int timestamp; // epoch 毫秒

  const LogEntry({
    required this.level,
    required this.tag,
    required this.message,
    required this.timestamp,
  });

  factory LogEntry.fromEvent(model.RsLogEvent e) => LogEntry(
        level: e.level,
        tag: e.tag,
        message: e.message,
        timestamp: DateTime.now().millisecondsSinceEpoch,
      );
}

/// 日志收集状态。
class LogState {
  final List<LogEntry> logs;
  /// 当前过滤级别。`null` = 全部；否则只显示 `>= filterLevel` 的日志。
  final model.RsLogLevel? filterLevel;

  const LogState({this.logs = const [], this.filterLevel});

  /// 过滤后的日志列表。
  List<LogEntry> get filtered {
    if (filterLevel == null) return logs;
    return logs.where((e) => e.level.index >= filterLevel!.index).toList();
  }

  LogState copyWith({
    List<LogEntry>? logs,
    model.RsLogLevel? filterLevel,
    bool clearFilter = false,
  }) =>
      LogState(
        logs: logs ?? this.logs,
        filterLevel: clearFilter ? null : (filterLevel ?? this.filterLevel),
      );
}

/// 日志收集管理。
///
/// 使用 [ReduxProvider]：`InstallLogCallbackAction` 注册 Rust 日志回调，
/// Rust 侧 `tracing` 事件通过 `StreamSink<RsLogEvent>` 推送，转 dispatch
/// [_AddLogAction]。`SetFilterAction` 切换过滤级别，`ClearLogsAction` 清空。
///
/// 最多保留 1000 条，超出后丢弃最旧的。
final logProvider = ReduxProvider<LogService, LogState>((ref) => LogService());

class LogService extends ReduxNotifier<LogState> {
  @override
  LogState init() => const LogState();

  StreamSubscription<model.RsLogEvent>? _sub;
}

const _kMaxLogs = 1000;

/// 注册 Rust 日志回调。应在 `RustLib.init()` 之后尽早调用。
class InstallLogCallbackAction extends AsyncReduxAction<LogService, LogState> {
  @override
  Future<LogState> reduce() async {
    if (notifier._sub != null) return state;
    try {
      final stream = rust_logging.setLogCallback();
      notifier._sub = stream.listen((e) {
        dispatch(_AddLogAction(LogEntry.fromEvent(e)));
      });
    } catch (e) {
      _logger.warning('setLogCallback failed: $e');
    }
    return state;
  }
}

/// 设置过滤级别。`null` 表示显示全部。
class SetLogFilterAction extends ReduxAction<LogService, LogState> {
  final model.RsLogLevel? level;

  SetLogFilterAction(this.level);

  @override
  LogState reduce() {
    if (level == null) return state.copyWith(clearFilter: true);
    return state.copyWith(filterLevel: level);
  }
}

/// 清空所有日志。
class ClearLogsAction extends ReduxAction<LogService, LogState> {
  @override
  LogState reduce() => const LogState();
}

/// 内部：添加一条日志。
class _AddLogAction extends ReduxAction<LogService, LogState> {
  final LogEntry entry;

  _AddLogAction(this.entry);

  @override
  LogState reduce() {
    final updated = [...state.logs, entry];
    // 超出上限则丢弃最旧的。
    final trimmed = updated.length > _kMaxLogs ? updated.sublist(updated.length - _kMaxLogs) : updated;
    return state.copyWith(logs: trimmed);
  }
}