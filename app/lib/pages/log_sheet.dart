import 'dart:async';

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
  unawaited(showModalBottomSheet(
    context: context,
    isScrollControlled: true,
    shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(20))),
    builder: (_) => const LogSheet(),
  ));
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
                : _LogList(logs: logs),
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

class _LogList extends StatefulWidget {
  final List<LogEntry> logs;
  const _LogList({required this.logs, super.key});

  @override
  State<_LogList> createState() => _LogListState();
}

class _LogListState extends State<_LogList> {
  final _controller = ScrollController();

  @override
  void didUpdateWidget(_LogList oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.logs.length != oldWidget.logs.length && widget.logs.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_controller.hasClients) {
          _controller.animateTo(
            _controller.position.maxScrollExtent,
            duration: const Duration(milliseconds: 150),
            curve: Curves.easeOut,
          );
        }
      });
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ListView.builder(
      controller: _controller,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      itemCount: widget.logs.length,
      itemBuilder: (context, i) {
        final entry = widget.logs[i];
        return _LogRow(
          entry: entry,
          onLongPress: () {
            final text = _formatEntry(entry);
            Clipboard.setData(ClipboardData(text: text));
            ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(t.log.copied)));
          },
        );
      },
    );
  }
}

class _LogRow extends StatelessWidget {
  final LogEntry entry;
  final VoidCallback onLongPress;

  const _LogRow({required this.entry, required this.onLongPress});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final time = DateTime.fromMillisecondsSinceEpoch(entry.timestamp);
    final timeStr = '${time.hour.toString().padLeft(2, '0')}:'
        '${time.minute.toString().padLeft(2, '0')}:'
        '${time.second.toString().padLeft(2, '0')}';
    final levelColor = _levelColor(entry.level, scheme);
    final levelInitial = entry.level.name[0].toUpperCase();

    return GestureDetector(
      onLongPress: onLongPress,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 1),
        child: Text.rich(
          TextSpan(
            style: const TextStyle(fontSize: 10, fontFamily: 'monospace', height: 1.3),
            children: [
              TextSpan(text: '$timeStr ', style: TextStyle(color: scheme.onSurfaceVariant)),
              TextSpan(text: levelInitial, style: TextStyle(color: levelColor, fontWeight: FontWeight.bold)),
              const TextSpan(text: ' '),
              TextSpan(text: '${entry.tag}: ', style: TextStyle(color: scheme.onSurfaceVariant)),
              TextSpan(text: entry.message, style: TextStyle(color: scheme.onSurface)),
            ],
          ),
          maxLines: 5,
          overflow: TextOverflow.ellipsis,
        ),
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