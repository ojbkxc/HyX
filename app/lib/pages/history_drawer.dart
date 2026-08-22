import 'dart:async';

import 'package:flutter/material.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/provider/history_provider.dart';
import 'package:hyx_app/util/formatters.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:refena_flutter/refena_flutter.dart';

/// 历史记录侧边栏内容。
///
/// 对应 Kotlin `HistoryScreen`：每条记录显示方向箭头、文件名、状态、大小、
/// 对端地址、耗时与时间戳。提供清空按钮。
///
/// 简化设计：作为 Drawer 的内容展示，不占主页面。
class HistoryDrawer extends StatelessWidget {
  const HistoryDrawer({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final state = context.watch(historyProvider);
    final records = state.records;

    return Column(
      children: [
        // 头部。
        Container(
          padding: const EdgeInsets.fromLTRB(20, 16, 8, 12),
          child: Row(
            children: [
              Icon(Icons.history, color: scheme.primary),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  t.history.title,
                  style: const TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
                ),
              ),
              if (records.isNotEmpty)
                IconButton(
                  icon: const Icon(Icons.delete_sweep),
                  tooltip: t.history.clear,
                  onPressed: () => _confirmClear(context),
                ),
            ],
          ),
        ),
        const Divider(height: 1),
        // 列表。
        Expanded(
          child: records.isEmpty
              ? _EmptyHistory()
              : ListView.builder(
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  itemCount: records.length,
                  itemBuilder: (_, i) => _HistoryCard(record: records[i]),
                ),
        ),
      ],
    );
  }

  void _confirmClear(BuildContext context) {
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(t.history.clear),
        content: Text(t.history.clearConfirm),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: Text(t.history.cancel)),
          TextButton(
            onPressed: () {
              unawaited(context.redux(historyProvider).dispatch(ClearHistoryAction()));
              Navigator.pop(ctx);
            },
            child: Text(t.history.clear),
          ),
        ],
      ),
    );
  }
}

class _EmptyHistory extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.history, size: 48, color: scheme.onSurfaceVariant.withValues(alpha: 0.4)),
          const SizedBox(height: 12),
          Text(t.history.empty, style: TextStyle(color: scheme.onSurfaceVariant)),
        ],
      ),
    );
  }
}

class _HistoryCard extends StatelessWidget {
  final HistoryRecord record;

  const _HistoryCard({required this.record});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final isSend = record.direction == model.RsTransferDirection.send;
    final statusColor = switch (record.status) {
      model.RsTransferStatus.completed => Colors.green,
      model.RsTransferStatus.failed => scheme.error,
      model.RsTransferStatus.cancelled => scheme.onSurfaceVariant,
      _ => Colors.amber,
    };
    final time = DateTime.fromMillisecondsSinceEpoch(record.timestamp);
    final timeStr = '${time.month.toString().padLeft(2, '0')}-${time.day.toString().padLeft(2, '0')} '
        '${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}';

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Row(
          children: [
            // 方向圆形图标。
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: statusColor.withValues(alpha: 0.15),
                shape: BoxShape.circle,
              ),
              alignment: Alignment.center,
              child: Text(
                isSend ? '↑' : '↓',
                style: TextStyle(color: statusColor, fontWeight: FontWeight.bold, fontSize: 18),
              ),
            ),
            const SizedBox(width: 12),
            // 文件信息。
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    record.fileName,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(height: 3),
                  Text(
                    '${formatBytes(record.bytesTransferred)} · '
                    '${record.peerAddress} · '
                    '${formatDuration(Duration(milliseconds: record.durationMillis))}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 11, color: scheme.onSurfaceVariant),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    timeStr,
                    style: TextStyle(fontSize: 10, color: scheme.outline),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 8),
            // 状态徽章。
            _StatusBadge(status: record.status),
          ],
        ),
      ),
    );
  }
}

class _StatusBadge extends StatelessWidget {
  final model.RsTransferStatus status;

  const _StatusBadge({required this.status});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (label, color, fg) = switch (status) {
      model.RsTransferStatus.completed => (t.transfer.completed, Colors.green, Colors.white),
      model.RsTransferStatus.failed => (t.transfer.failed, scheme.error, Colors.white),
      model.RsTransferStatus.cancelled => (t.transfer.cancelled, scheme.outline, scheme.onSurface),
      _ => (t.transfer.interrupted, Colors.amber, Colors.white),
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(color: color, borderRadius: BorderRadius.circular(10)),
      child: Text(label, style: TextStyle(fontSize: 10, color: fg, fontWeight: FontWeight.w500)),
    );
  }
}