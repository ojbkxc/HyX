import 'dart:math' as math;

import 'package:flutter/material.dart';

/// 极简环形进度组件。
///
/// 对应 Kotlin `RingProgress.kt`：底色为半透明灰环，前景为圆角绿色弧。
/// 通过 [CustomPainter] 直接绘制，避免 AnimatedBuilder 的额外开销；
/// 进度变化由父 Widget 通过 [Tween] 隐式动画驱动（这里使用 [AnimatedBuilder] +
/// [AnimationController] 的简化版：直接重绘即可，5Hz 节流已由 Rust 侧完成）。
class RingProgress extends StatelessWidget {
  /// 进度，0..1，超出范围会被夹取。
  final double progress;

  /// 直径。
  final double size;

  /// 底环厚度。
  final double trackThickness;

  /// 进度弧厚度。
  final double progressThickness;

  /// 进度色，默认取主题 `primary`。
  final Color? progressColor;

  /// 底环色，默认取主题 `outlineVariant`。
  final Color? trackColor;

  /// 是否在中心显示百分比文字。
  final bool showPercentLabel;

  const RingProgress({
    required this.progress,
    this.size = 140,
    this.trackThickness = 10,
    this.progressThickness = 10,
    this.progressColor,
    this.trackColor,
    this.showPercentLabel = true,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final fg = progressColor ?? scheme.primary;
    final bg = trackColor ?? scheme.outlineVariant.withValues(alpha: 0.35);
    final clamped = progress.clamp(0.0, 1.0);

    return SizedBox(
      width: size,
      height: size,
      child: CustomPaint(
        painter: _RingPainter(
          progress: clamped,
          trackColor: bg,
          progressColor: fg,
          trackThickness: trackThickness,
          progressThickness: progressThickness,
        ),
        child: showPercentLabel
            ? Center(
                child: Text(
                  '${(clamped * 100).toInt()}%',
                  style: TextStyle(
                    fontSize: size * 0.18,
                    fontWeight: FontWeight.bold,
                    color: scheme.onSurface,
                  ),
                ),
              )
            : null,
      ),
    );
  }
}

class _RingPainter extends CustomPainter {
  final double progress;
  final Color trackColor;
  final Color progressColor;
  final double trackThickness;
  final double progressThickness;

  _RingPainter({
    required this.progress,
    required this.trackColor,
    required this.progressColor,
    required this.trackThickness,
    required this.progressThickness,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final center = Offset(size.width / 2, size.height / 2);
    // 半径取较细那一笔为基准，确保两环都在画布内。
    final stroke = math.max(trackThickness, progressThickness);
    final radius = (math.min(size.width, size.height) - stroke) / 2;

    // 底环。
    canvas.drawCircle(
      center,
      radius,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = trackThickness
        ..color = trackColor
        ..strokeCap = StrokeCap.round,
    );

    // 进度弧：从 -90°（顶部）顺时针扫过 progress * 360°。
    if (progress > 0) {
      canvas.drawArc(
        Rect.fromCircle(center: center, radius: radius),
        -math.pi / 2,
        progress * 2 * math.pi,
        false,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = progressThickness
          ..color = progressColor
          ..strokeCap = StrokeCap.round,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _RingPainter old) =>
      old.progress != progress ||
      old.trackColor != trackColor ||
      old.progressColor != progressColor ||
      old.trackThickness != trackThickness ||
      old.progressThickness != progressThickness;
}