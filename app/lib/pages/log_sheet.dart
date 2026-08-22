import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/provider/log_provider.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:refena_flutter/refena_flutter.dart';

/// 日志查看器（ModalBottomSheet）。
///
/// 对应 Kotlin `LogSheet.kt`：级别过滤 chips + 等宽字体日志列表 +
/// 复制/清空/导出。
void showLogSheet(BuildContext context) {
  showModalBottomSheet(
    context: context,
    isScrollControlled: true,
    shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(20))),
    builder: (_) => const LogSheet(),
  );
}

class LogSheet extends StatelessWidget {
  const LogSheet({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final state = context.watch(logProvider);
    final logs = state.filtered;

    return DraggableScrollableSheet(
      initialChildSize: 0.85,
      minChildSize: 0.4,
      maxChildSize: 0.95,
      expand: false,
      builder: (_, controller) => Column(
        children: [
          // 头部。
          Container(
            padding: const EdgeInsets.fromLTRB(16, 12, 8, 8),
            child: Row(
              children: [
                Icon(Icons.article, color: scheme.primary, size: 20),
                const SizedBox(width: 8),
                Text(t.log.title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
                const Spacer(),
                IconButton(
                  icon: const Icon(Icons.copy),
                  tooltip: t.log.copy,
                  onPressed: () => _copyAll(context, state.logs),
                ),
                IconButton(
                  icon: const Icon(Icons.delete_sweep),
                  tooltip: t.log.clear,
                  onPressed: () => context.redux(logProvider).dispatch(ClearLogsAction()),
                ),
              ],
            ),
          ),
          // 级别过滤。
          _LevelFilterRow(active: state.filterLevel),
          const Divider(height: 1),
          // 日志列表。
          Expanded(
            child: logs.isEmpty
                ? Center(
                    child: Text(t.log.empty, style: TextStyle(color: scheme.onSurfaceVariant)),
                  )
                : ListView.builder(
                    controller: controller,
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                    itemCount: logs.length,
                    itemBuilder: (_, i) => _LogRow(entry: logs[i]),
                  ),
          ),
        ],
      ),
    );
  }

  Future<void> _copyAll(BuildContext context, List<LogEntry> logs) async {
    if (logs.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(t.log.empty)));
      return;
    }
    final text = logs.map(_formatEntry).join('\n');
    await Clipboard.setData(ClipboardData(text: text));
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(t.log.copied)));
    }
  }
}

String _formatEntry(LogEntry e) {
  final time = DateTime.fromMillisecondsSinceEpoch(e.timestamp);
  final timeStr = '${time.hour.toString().padLeft(2, '0')}:'
      '${time.minute.toString().padLeft(2, '0')}:'
      '${time.second.toString().padLeft(2, '0')}.'
      '${time.millisecond.toString().padLeft(3, '0')}';
  return '$timeStr [${e.level.name.toUpperCase()}] ${e.tag}: ${e.message}';
}

class _LevelFilterRow extends StatelessWidget {
  final model.RsLogLevel? active;

  const _LevelFilterRow({required this.active});

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 44,
      child: ListView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        children: [
          FilterChip(
            selected: active == null,
            onSelected: (_) => context.redux(logProvider).dispatch(SetLogFilterAction(null)),
            label: Text(t.log.filterAll),
          ),
          const SizedBox(width: 8),
          for (final level in model.RsLogLevel.values) ...[
            FilterChip(
              selected: active == level,
              onSelected: (_) => context.redux(logProvider).dispatch(
                SetLogFilterAction(active == level ? null : level),
              ),
              label: Text(level.name.toUpperCase()),
            ),
            const SizedBox(width: 8),
          ],
        ],
      ),
    );
  }
}

class _LogRow extends StatelessWidget {
  final LogEntry entry;

  const _LogRow({required this.entry});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final time = DateTime.fromMillisecondsSinceEpoch(entry.timestamp);
    final timeStr = '${time.hour.toString().padLeft(2, '0')}:'
        '${time.minute.toString().padLeft(2, '0')}:'
        '${time.second.toString().padLeft(2, '0')}.'
        '${time.millisecond.toString().padLeft(3, '0')}';
    final levelColor = _levelColor(entry.level, scheme);

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 2),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(
                timeStr,
                style: TextStyle(fontSize: 10, fontFamily: 'monospace', color: scheme.onSurfaceVariant),
              ),
              const SizedBox(width: 6),
              Text(
                '[${entry.level.name.toUpperCase()}]',
                style: TextStyle(fontSize: 10, fontFamily: 'monospace', fontWeight: FontWeight.bold, color: levelColor),
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  entry.tag,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 10, fontFamily: 'monospace', color: scheme.onSurfaceVariant),
                ),
              ),
            ],
          ),
          Text(
            entry.message,
            style: TextStyle(fontSize: 11, fontFamily: 'monospace', color: scheme.onSurface),
          ),
        ],
      ),
    );
  }

  Color _levelColor(model.RsLogLevel level, ColorScheme scheme) {
    switch (level) {
      case model.RsLogLevel.trace:
      case model.RsLogLevel.debug:
        return scheme.onSurfaceVariant;
      case model.RsLogLevel.info:
        return Colors.green;
      case model.RsLogLevel.warn:
        return Colors.amber;
      case model.RsLogLevel.error:
        return scheme.error;
    }
  }
}