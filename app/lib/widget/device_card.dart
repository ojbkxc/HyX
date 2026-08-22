import 'package:flutter/material.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;

/// 设备卡片。
///
/// 对应 Kotlin `DevicesScreen.OnlineDeviceCard`：左侧圆形头像 + 中间名称/地址 +
/// 右侧在线状态徽章。点击触发文件选择；桌面端拖拽由父级 [DropTarget] 统一处理，
/// 此组件仅暴露 [onTap] 与 [onDropFiles] 回调。
///
/// 简化设计：去掉 `allowTransfer` 切换按钮——HyX 默认 `allowTransfer=true`，
/// 自动接收，不再弹确认框（参见 plan.md "比 LocalSend 更简单"）。
class DeviceCard extends StatelessWidget {
  /// 待渲染的 peer。`RsDiscoveredPeer` 仅含 name/addr/deviceId，故 `online` 恒真。
  final model.RsDiscoveredPeer peer;

  /// 点击卡片：触发文件选择 → 发送。
  final VoidCallback onTap;

  /// 拖拽完成（桌面端）。传入拖入的文件路径列表。
  /// 移动端该回调为 null，卡片不响应拖拽。
  final void Function(List<String> paths)? onDropFiles;

  /// 是否处于拖拽悬停态（桌面端），用于高亮卡片。
  final bool dragHover;

  const DeviceCard({
    required this.peer,
    required this.onTap,
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
        onTap: onTap,
        borderRadius: BorderRadius.circular(16),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
          child: Row(
            children: [
              _Avatar(name: peer.name),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      peer.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      peer.addr,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 12, color: scheme.onSurfaceVariant),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              _OnlineBadge(),
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
      decoration: BoxDecoration(color: scheme.surfaceVariant, shape: BoxShape.circle),
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

/// 在线状态徽章：绿色圆点 + "在线" 文本。
class _OnlineBadge extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: scheme.primaryContainer,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 6,
            height: 6,
            decoration: BoxDecoration(color: scheme.primary, shape: BoxShape.circle),
          ),
          const SizedBox(width: 4),
          Text(t.home.online, style: TextStyle(fontSize: 11, color: scheme.onPrimaryContainer)),
        ],
      ),
    );
  }
}