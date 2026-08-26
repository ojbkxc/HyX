import 'package:flutter/material.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/provider/device_provider.dart';

/// 设备卡片（轻量版）。
///
/// 仅展示设备身份与在线状态：
/// - 在线设备（[online]=true）：正常色调，点击触发文件选择 → 发送。
/// - 历史设备（[online]=false）：整体置灰，点击不响应；可滑动删除（由父级
///   [HomePage] 用 [Dismissible] 包裹实现）。
///
/// 桌面端拖拽由父级 [DropTarget] 统一处理，此组件仅暴露 [onTap] 与
/// [onDropFiles] 回调。
class DeviceCard extends StatelessWidget {
  /// 待渲染的设备（含 deviceId/name/addr/allowReceive）。
  final KnownDevice device;

  /// 是否在线。false 时卡片置灰且不响应点击。
  final bool online;

  /// 点击卡片：触发文件选择 → 发送。null 表示不可点击（历史设备）。
  final VoidCallback? onTap;

  /// 拖拽完成（桌面端）。传入拖入的文件路径列表。
  /// 移动端该回调为 null，卡片不响应拖拽。
  final void Function(List<String> paths)? onDropFiles;

  /// 是否处于拖拽悬停态（桌面端），用于高亮卡片。
  final bool dragHover;

  const DeviceCard({
    required this.device,
    required this.online,
    this.onTap,
    this.onDropFiles,
    this.dragHover = false,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Card(
      shape: dragHover
          ? RoundedRectangleBorder(
              side: BorderSide(color: scheme.primary, width: 2),
              borderRadius: BorderRadius.circular(16),
            )
          : RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
      color: dragHover ? scheme.primaryContainer : scheme.surface,
      child: InkWell(
        onTap: online ? onTap : null,
        borderRadius: BorderRadius.circular(16),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              // 上半部分：头像 + 名称/地址 + 状态徽章。
              Opacity(
                opacity: online ? 1.0 : 0.5,
                child: Row(
                  children: [
                    _Avatar(name: device.name),
                    const SizedBox(width: 14),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            device.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
                          ),
                          const SizedBox(height: 4),
                          Text(
                            device.addr,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(fontSize: 12, color: scheme.onSurfaceVariant),
                          ),
                        ],
                      ),
                    ),
                    const SizedBox(width: 8),
                    _StatusBadge(online: online),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 圆形头像：取名称首字母（大写）显示，背景取主题 `surfaceVariant`。
class _Avatar extends StatelessWidget {
  final String name;

  const _Avatar({required this.name});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final initial = name.isEmpty ? '?' : name[0].toUpperCase();
    return Container(
      width: 44,
      height: 44,
      decoration: BoxDecoration(color: scheme.surfaceContainerHighest, shape: BoxShape.circle),
      alignment: Alignment.center,
      child: Text(
        initial,
        style: TextStyle(
          fontSize: 18,
          fontWeight: FontWeight.bold,
          color: scheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

/// 状态徽章：在线时绿色"在线"，离线时灰色"离线"。
class _StatusBadge extends StatelessWidget {
  final bool online;

  const _StatusBadge({required this.online});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final color = online ? scheme.primary : scheme.outline;
    final bg = online ? scheme.primaryContainer : scheme.surfaceContainerHighest;
    final fg = online ? scheme.onPrimaryContainer : scheme.onSurfaceVariant;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 6,
            height: 6,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle),
          ),
          const SizedBox(width: 4),
          Text(
            online ? t.home.online : t.devices.offline,
            style: TextStyle(fontSize: 11, color: fg),
          ),
        ],
      ),
    );
  }
}
