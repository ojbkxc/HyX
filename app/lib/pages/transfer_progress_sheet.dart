import 'dart:async';

import 'package:flutter/material.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/provider/history_provider.dart';
import 'package:hyx_app/provider/transfer_provider.dart';
import 'package:hyx_app/util/formatters.dart';
import 'package:hyx_app/widget/ring_progress.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:refena_flutter/refena_flutter.dart';

/// 传输进度浮层（BottomSheet）。
///
/// 对应 Kotlin `TransferScreen.TransferringPanel`：环形进度 + 速度 + 已传/总量 +
/// 耗时 + 取消按钮。完成后自动关闭并展示成功/失败 SnackBar。
///
/// 简化设计：作为 showModalBottomSheet 弹出，传输结束后延迟 1.5s 自动关闭，
/// 让用户看到最终状态。
void showTransferProgressSheet(BuildContext context) {
  showModalBottomSheet(
    context: context,
    isScrollControlled: true,
    isDismissible: false,
    enableDrag: false,
    shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(20))),
    builder: (_) => const TransferProgressSheet(),
  );
}

class TransferProgressSheet extends StatefulWidget {
  const TransferProgressSheet({super.key});

  @override
  State<TransferProgressSheet> createState() => _TransferProgressSheetState();
}

class _TransferProgressSheetState extends State<TransferProgressSheet> with Refena {
  bool _autoCloseScheduled = false;

  @override
  Widget build(BuildContext context) {
    final st = context.watch(transferProvider);
    final scheme = Theme.of(context).colorScheme;

    // 终态后安排自动关闭。
    if (st.done && !_autoCloseScheduled) {
      _autoCloseScheduled = true;
      _scheduleAutoClose(st);
    }

    final elapsed = st.startTime != null
        ? DateTime.now().difference(st.startTime!)
        : Duration.zero;
    final eta = etaMillis(st.transferred, st.total, st.speed);

    return Padding(
      padding: EdgeInsets.only(
        left: 24,
        right: 24,
        top: 20,
        bottom: 24 + MediaQuery.of(context).viewInsets.bottom,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          // 拖把。
          Container(
            width: 40,
            height: 4,
            margin: const EdgeInsets.only(bottom: 16),
            decoration: BoxDecoration(color: scheme.outlineVariant, borderRadius: BorderRadius.circular(2)),
          ),
          // 标题：文件名 + 方向。
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(
                st.direction == model.RsTransferDirection.send ? Icons.arrow_upward : Icons.arrow_downward,
                size: 18,
                color: scheme.primary,
              ),
              const SizedBox(width: 6),
              Text(
                st.fileName.isEmpty ? t.transfer.inProgress : st.fileName,
                style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ],
          ),
          const SizedBox(height: 20),
          // 环形进度。
          RingProgress(progress: st.fraction, size: 160),
          const SizedBox(height: 20),
          // 状态文本。
          _StatusText(status: st.status, message: st.errorMessage),
          const SizedBox(height: 16),
          // 指标行。
          _MetricRow(
            transferred: st.transferred,
            total: st.total,
            speed: st.speed,
            elapsed: elapsed,
            etaMillis: eta,
          ),
          const SizedBox(height: 20),
          // 取消/完成按钮。
          if (st.busy)
            OutlinedButton.icon(
              onPressed: () => context.redux(transferProvider).dispatch(CancelTransferAction()),
              icon: const Icon(Icons.close),
              label: Text(t.transfer.cancel),
            )
          else
            FilledButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(t.transfer.close),
            ),
        ],
      ),
    );
  }

  void _scheduleAutoClose(TransferState st) {
    Future.delayed(const Duration(milliseconds: 1500), () {
      if (!mounted) return;
      // 添加历史记录。
      if (st.startTime != null && st.endTime != null) {
        final record = HistoryRecord(
          id: '${st.startTime!.millisecondsSinceEpoch}',
          direction: st.direction,
          fileName: st.fileName,
          status: st.status,
          bytesTransferred: st.transferred,
          peerAddress: st.peerAddress,
          durationMillis: st.endTime!.difference(st.startTime!).inMilliseconds,
          timestamp: st.endTime!.millisecondsSinceEpoch,
        );
        unawaited(context.redux(historyProvider).dispatchAsync(AddHistoryRecordAction(record)));
      }
      // 展示结果 SnackBar 后关闭。
      final msg = st.status == model.RsTransferStatus.completed
          ? t.transfer.completed
          : st.status == model.RsTransferStatus.failed
              ? '${t.transfer.failed}: ${st.errorMessage ?? ''}'
              : t.transfer.cancelled;
      final ok = st.status == model.RsTransferStatus.completed;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(msg),
          backgroundColor: ok ? Colors.green.shade700 : Colors.red.shade700,
          duration: const Duration(seconds: 2),
        ),
      );
      Navigator.of(context).pop();
      // 重置状态，供下次传输。
      context.redux(transferProvider).dispatch(ResetTransferAction());
    });
  }
}

/// 状态文本：根据 status 显示不同颜色。
class _StatusText extends StatelessWidget {
  final model.RsTransferStatus status;
  final String? message;

  const _StatusText({required this.status, this.message});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (text, color) = switch (status) {
      model.RsTransferStatus.idle => (t.transfer.idle, scheme.onSurfaceVariant),
      model.RsTransferStatus.pairing => (t.transfer.pairing, scheme.tertiary),
      model.RsTransferStatus.connecting => (t.transfer.connecting, scheme.tertiary),
      model.RsTransferStatus.transferring => (t.transfer.transferring, scheme.primary),
      model.RsTransferStatus.completed => (t.transfer.completed, Colors.green),
      model.RsTransferStatus.failed => ('${t.transfer.failed}${message != null ? ': $message' : ''}', scheme.error),
      model.RsTransferStatus.cancelled => (t.transfer.cancelled, scheme.onSurfaceVariant),
    };
    return Text(
      text,
      style: TextStyle(fontSize: 14, color: color, fontWeight: FontWeight.w500),
      textAlign: TextAlign.center,
      maxLines: 2,
      overflow: TextOverflow.ellipsis,
    );
  }
}

/// 指标行：已传/总量、速度、耗时、剩余。
class _MetricRow extends StatelessWidget {
  final int transferred;
  final int total;
  final double speed;
  final Duration elapsed;
  final int etaMillis;

  const _MetricRow({
    required this.transferred,
    required this.total,
    required this.speed,
    required this.elapsed,
    required this.etaMillis,
  });

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final labelStyle = TextStyle(fontSize: 11, color: scheme.onSurfaceVariant);
    final valueStyle = const TextStyle(fontSize: 14, fontWeight: FontWeight.w600);
    return Row(
      children: [
        _Metric(label: t.transfer.transferred, value: formatBytes(transferred), labelStyle: labelStyle, valueStyle: valueStyle),
        _Metric(label: t.transfer.total, value: formatBytes(total), labelStyle: labelStyle, valueStyle: valueStyle),
        _Metric(label: t.transfer.speed, value: formatSpeed(speed), labelStyle: labelStyle, valueStyle: valueStyle),
        _Metric(label: t.transfer.elapsed, value: formatDuration(elapsed), labelStyle: labelStyle, valueStyle: valueStyle),
        _Metric(
          label: t.transfer.remaining,
          value: etaMillis > 0 ? formatDuration(Duration(milliseconds: etaMillis)) : '—',
          labelStyle: labelStyle,
          valueStyle: valueStyle,
        ),
      ],
    );
  }
}

class _Metric extends StatelessWidget {
  final String label;
  final String value;
  final TextStyle labelStyle;
  final TextStyle valueStyle;

  const _Metric({
    required this.label,
    required this.value,
    required this.labelStyle,
    required this.valueStyle,
  });

  @override
  Widget build(BuildContext context) {
    return Expanded(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(label, style: labelStyle),
          const SizedBox(height: 2),
          Text(value, style: valueStyle, maxLines: 1, overflow: TextOverflow.ellipsis),
        ],
      ),
    );
  }
}