import 'dart:convert';

import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:logging/logging.dart';
import 'package:refena_flutter/refena_flutter.dart';
import 'package:shared_preferences/shared_preferences.dart';

final _logger = Logger('HistoryProvider');

const _kPrefKey = 'hyx_history';
const _kMaxRecords = 50;

/// 历史记录条目。
///
/// 对应 Kotlin `HistoryRecord`：记录一次传输的方向、文件名、状态、大小、
/// 对端地址、耗时与时间戳。持久化到 SharedPreferences（JSON 编码）。
class HistoryRecord {
  final String id;
  final model.RsTransferDirection direction;
  final String fileName;
  final model.RsTransferStatus status;
  final int bytesTransferred;
  final String peerAddress;
  final int durationMillis;
  final int timestamp; // epoch 毫秒

  const HistoryRecord({
    required this.id,
    required this.direction,
    required this.fileName,
    required this.status,
    required this.bytesTransferred,
    required this.peerAddress,
    required this.durationMillis,
    required this.timestamp,
  });

  Map<String, dynamic> toJson() => {
        'id': id,
        'direction': direction.name,
        'fileName': fileName,
        'status': status.name,
        'bytes': bytesTransferred,
        'peer': peerAddress,
        'duration': durationMillis,
        'timestamp': timestamp,
      };

  static HistoryRecord fromJson(Map<String, dynamic> json) => HistoryRecord(
        id: json['id'] as String,
        direction: model.RsTransferDirection.values.byName(json['direction'] as String),
        fileName: json['fileName'] as String,
        status: model.RsTransferStatus.values.byName(json['status'] as String),
        bytesTransferred: (json['bytes'] as num).toInt(),
        peerAddress: json['peer'] as String,
        durationMillis: (json['duration'] as num).toInt(),
        timestamp: (json['timestamp'] as num).toInt(),
      );
}

/// 历史记录状态。
class HistoryState {
  final List<HistoryRecord> records;

  const HistoryState({this.records = const []});
}

/// 历史记录管理。
///
/// 使用 [ReduxProvider]：`AddRecordAction` 添加并持久化，`ClearHistoryAction` 清空。
/// `init` 时从 SharedPreferences 加载已有记录。
final historyProvider = ReduxProvider<HistoryService, HistoryState>((ref) => HistoryService());

class HistoryService extends ReduxNotifier<HistoryState> {
  @override
  HistoryState init() {
    // 异步加载已有记录（init 返回后执行）。
    unawaited(dispatchAsync(const _LoadHistoryAction()));
    return const HistoryState();
  }

  Future<void> _persist(List<HistoryRecord> records) async {
    try {
      final prefs = await SharedPreferences.getInstance();
      final raw = jsonEncode(records.map((r) => r.toJson()).toList());
      await prefs.setString(_kPrefKey, raw);
    } catch (e) {
      _logger.warning('persist history failed: $e');
    }
  }
}

/// 内部：从 SharedPreferences 异步加载历史记录。
class _LoadHistoryAction extends AsyncReduxAction<HistoryService, HistoryState> {
  const _LoadHistoryAction();

  @override
  Future<HistoryState> reduce() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      final raw = prefs.getString(_kPrefKey);
      if (raw == null) return state;
      final list = jsonDecode(raw) as List<dynamic>;
      final records = list
          .map((e) => HistoryRecord.fromJson(e as Map<String, dynamic>))
          .toList();
      return HistoryState(records: records);
    } catch (e) {
      _logger.warning('load history failed: $e');
      return state;
    }
  }
}

/// 添加一条历史记录。新记录插入头部，超过 [_kMaxRecords] 截断。
class AddHistoryRecordAction extends AsyncReduxAction<HistoryService, HistoryState> {
  final HistoryRecord record;

  AddHistoryRecordAction(this.record);

  @override
  Future<HistoryState> reduce() async {
    final updated = [record, ...state.records].take(_kMaxRecords).toList();
    await notifier._persist(updated);
    return HistoryState(records: updated);
  }
}

/// 清空所有历史记录。
class ClearHistoryAction extends AsyncReduxAction<HistoryService, HistoryState> {
  @override
  Future<HistoryState> reduce() async {
    await notifier._persist([]);
    return const HistoryState();
  }
}