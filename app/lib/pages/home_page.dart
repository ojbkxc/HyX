import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/pages/history_drawer.dart';
import 'package:hyx_app/pages/log_sheet.dart';
import 'package:hyx_app/pages/pairing_dialog.dart';
import 'package:hyx_app/pages/transfer_progress_sheet.dart';
import 'package:hyx_app/provider/device_provider.dart';
import 'package:hyx_app/provider/log_provider.dart';
import 'package:hyx_app/provider/transfer_provider.dart';
import 'package:hyx_app/util/update_checker.dart';
import 'package:hyx_app/widget/device_card.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:refena_flutter/refena_flutter.dart';

/// HyX 主页面。
///
/// 设备列表分两区显示：
/// - **在线设备**：当前局域网发现的设备，正常色调，点击发送文件。
/// - **历史设备**：曾经遇到但当前不在线的设备，置灰显示，可滑动删除。
///
/// 每个设备底部都有接收/禁止切换按钮，控制是否允许接收来自此设备的文件。
/// 状态用 device_id（Uuid）唯一标识并持久化到 SharedPreferences。
///
/// 应用启动时自动开始接收监听（[StartAutoListenAction]），无需手动点 FAB，
/// 修复"手机到电脑传不了"的问题。FAB 改为查看接收状态。
class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> with Refena {
  /// 传输进度浮层是否已打开，避免重复弹出。
  bool _sheetOpen = false;

  @override
  void initState() {
    super.initState();
    ensureRef((ref) async {
      // 注册日志回调。
      await ref.redux(logProvider).dispatchAsync(InstallLogCallbackAction());
      // 加载本设备身份（fire-and-forget）。
      unawaited(ref.redux(deviceProvider).dispatchAsync(LoadMyDeviceAction()));
      // 加载持久化的已知设备列表（含历史设备 + 接收/禁止状态）。
      unawaited(ref.redux(deviceProvider).dispatchAsync(LoadKnownDevicesAction()));
      // 启动自动发现。
      ref.redux(deviceProvider).dispatch(StartDiscoveryAction());
      // 自动启动接收监听（不需要手动点 FAB），修复手机到电脑传输问题。
      unawaited(ref.redux(transferProvider).dispatchAsync(StartAutoListenAction()));
      // 检测更新（fire-and-forget）。
      unawaited(_checkForUpdate());
    });
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final devState = context.watch(deviceProvider);
    final transferState = context.watch(transferProvider);

    // 当收到传输（status 变为 transferring）时自动弹出进度浮层。
    // 仅在浮层未打开时调度，由 [_showTransferSheet] 内部 guard 保证只弹一次。
    if (transferState.status == model.RsTransferStatus.transferring && !_sheetOpen) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        _showTransferSheet();
      });
    }

    return Scaffold(
      appBar: AppBar(
        title: Row(
          children: [
            Text(t.appName, style: const TextStyle(fontWeight: FontWeight.bold)),
            const SizedBox(width: 8),
            if (devState.scanning)
              const SizedBox(
                width: 14,
                height: 14,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
          ],
        ),
        actions: [
          // 手动刷新。
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: t.home.refresh,
            onPressed: () => unawaited(context.redux(deviceProvider).dispatchAsync(RefreshPeersAction())),
          ),
          // 日志。
          IconButton(
            icon: const Icon(Icons.article),
            tooltip: t.log.title,
            onPressed: () => showLogSheet(context),
          ),
        ],
      ),
      drawer: _buildDrawer(context),
      body: _buildBody(context, devState),
      // FAB：查看接收状态 / 传输进度。
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => _showTransferSheet(),
        icon: transferState.busy
            ? const SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white),
              )
            : const Icon(Icons.wifi_tethering),
        label: Text(transferState.busy ? t.transfer.inProgress : t.home.startReceive),
        backgroundColor: scheme.primaryContainer,
        foregroundColor: scheme.onPrimaryContainer,
      ),
    );
  }

  /// 侧边栏 Drawer：本设备信息 + 历史记录 + 配对码入口 + 设置。
  Widget _buildDrawer(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final devState = context.watch(deviceProvider);
    final myDev = devState.myDevice;
    return Drawer(
      child: SafeArea(
        child: Column(
          children: [
            // 头部：本设备信息。
            UserAccountsDrawerHeader(
              accountName: Text(myDev?.name ?? 'HyX'),
              accountEmail: Text(myDev != null ? 'ID: ${myDev.id.toString().substring(0, 8)}' : ''),
              currentAccountPicture: CircleAvatar(
                backgroundColor: scheme.primaryContainer,
                child: Text(
                  (myDev?.name ?? 'H')[0].toUpperCase(),
                  style: TextStyle(fontSize: 24, color: scheme.onPrimaryContainer),
                ),
              ),
            ),
            // 历史记录（展开式）。
            Expanded(
              child: const HistoryDrawer(),
            ),
            const Divider(height: 1),
            // 底部操作。
            ListTile(
              leading: const Icon(Icons.qr_code),
              title: Text(t.pair.title),
              subtitle: Text(t.pair.subtitle),
              onTap: () {
                Navigator.pop(context); // 关闭 Drawer。
                unawaited(showPairingDialog(context));
              },
            ),
            ListTile(
              leading: const Icon(Icons.article),
              title: Text(t.log.title),
              onTap: () {
                Navigator.pop(context);
                showLogSheet(context);
              },
            ),
            ListTile(
              leading: const Icon(Icons.info_outline),
              title: Text(t.home.about),
              onTap: () {
                Navigator.pop(context);
                _showAbout(context);
              },
            ),
          ],
        ),
      ),
    );
  }

  /// 主内容：分区显示在线设备 + 历史设备。
  Widget _buildBody(BuildContext context, DeviceState devState) {
    final onlineDevices = devState.onlineDevices;
    final historyDevices = devState.historyDevices;

    // 两者都为空：显示空状态。
    if (onlineDevices.isEmpty && historyDevices.isEmpty) {
      return _EmptyState(scanning: devState.scanning);
    }

    final scheme = Theme.of(context).colorScheme;

    // 构建列表 children：在线区域 + 历史区域。
    final children = <Widget>[];

    // 在线设备区域。
    children.add(_SectionHeader(title: t.devices.onlineSection, count: onlineDevices.length));
    if (onlineDevices.isEmpty) {
      children.add(_EmptySectionHint(text: t.devices.emptyOnline));
    } else {
      for (final d in onlineDevices) {
        children.add(
          Padding(
            padding: const EdgeInsets.only(bottom: 10),
            child: DeviceCard(
              device: d,
              online: true,
              onTap: () => _sendToPeer(context, d),
              onToggleAllowReceive: () => _toggleAllow(context, d.deviceId),
            ),
          ),
        );
      }
    }

    // 历史设备区域（仅当非空时显示）。
    if (historyDevices.isNotEmpty) {
      children.add(const SizedBox(height: 8));
      children.add(_SectionHeader(title: t.devices.historySection, count: historyDevices.length));
      for (final d in historyDevices) {
        children.add(
          Padding(
            padding: const EdgeInsets.only(bottom: 10),
            child: Dismissible(
              key: ValueKey('history-${d.deviceId}'),
              direction: DismissDirection.endToStart,
              background: Container(
                alignment: Alignment.centerRight,
                padding: const EdgeInsets.symmetric(horizontal: 20),
                decoration: BoxDecoration(
                  color: scheme.error,
                  borderRadius: BorderRadius.circular(16),
                ),
                child: const Icon(Icons.delete, color: Colors.white),
              ),
              confirmDismiss: (_) => _confirmDelete(context, d.name),
              onDismissed: (_) {
                unawaited(context.redux(deviceProvider).dispatchAsync(RemoveKnownDeviceAction(d.deviceId)));
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(
                    content: Text(t.devices.deleted(d.name)),
                    duration: const Duration(seconds: 2),
                  ),
                );
              },
              child: DeviceCard(
                device: d,
                online: false,
                onToggleAllowReceive: () => _toggleAllow(context, d.deviceId),
              ),
            ),
          ),
        );
      }
    }

    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 96),
      children: children,
    );
  }

  /// 点击在线设备 → 选择文件 → 发送。
  Future<void> _sendToPeer(BuildContext context, KnownDevice device) async {
    final result = await FilePicker.pickFiles(allowMultiple: false);
    if (result == null || result.files.isEmpty) return;
    final path = result.files.first.path;
    if (path == null) return;

    if (!context.mounted) return;
    unawaited(context.redux(transferProvider).dispatchAsync(
      StartSendAction(peerAddress: device.addr, filePath: path),
    ));
    _showTransferSheet();
  }

  /// 切换设备的接收/禁止状态。
  void _toggleAllow(BuildContext context, String deviceId) {
    unawaited(context.redux(deviceProvider).dispatchAsync(ToggleAllowReceiveAction(deviceId)));
  }

  /// 弹出传输进度浮层。
  void _showTransferSheet() {
    if (_sheetOpen) return;
    _sheetOpen = true;
    unawaited(showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      isDismissible: false,
      enableDrag: false,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(20))),
      builder: (_) => const TransferProgressSheet(),
    ).then((_) {
      // 浮层关闭后重置标志，允许下次传输再次弹出。
      _sheetOpen = false;
    }));
  }

  /// 删除历史设备的确认对话框。
  Future<bool?> _confirmDelete(BuildContext context, String name) {
    return showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(t.devices.delete),
        content: Text(t.devices.deleteConfirm(name)),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: Text(t.history.cancel)),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            style: TextButton.styleFrom(foregroundColor: Theme.of(ctx).colorScheme.error),
            child: Text(t.devices.delete),
          ),
        ],
      ),
    );
  }

  void _showAbout(BuildContext context) {
    unawaited(showDialog(
      context: context,
      builder: (ctx) => AboutDialog(
        applicationName: t.appName,
        applicationVersion: '0.1.0',
        applicationLegalese: 'P2P file transfer over QUIC',
      ),
    ));
  }

  /// 检测应用更新，发现新版本时弹窗提示。
  ///
  /// 检测顺序：优先 R2 下载站 latest.json，回退到 GitHub Releases。
  /// 当前版本号先硬编码 '1.0.0'，后续可改为从 pubspec 读取。
  Future<void> _checkForUpdate() async {
    try {
      final info = await UpdateChecker.check('1.0.0');
      if (info != null && mounted) {
        unawaited(showDialog(
          context: context,
          builder: (context) => AlertDialog(
            title: Text('发现新版本 ${info.version}'),
            content: Text(info.body),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('稍后'),
              ),
              TextButton(
                onPressed: () {
                  Navigator.pop(context);
                  // 打开下载链接
                },
                child: const Text('立即更新'),
              ),
            ],
          ),
        ));
      }
    } catch (_) {}
  }
}

/// 分区标题：标题文本 + 设备数量徽章。
class _SectionHeader extends StatelessWidget {
  final String title;
  final int count;

  const _SectionHeader({required this.title, required this.count});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.fromLTRB(4, 12, 4, 8),
      child: Row(
        children: [
          Text(
            title,
            style: TextStyle(
              fontSize: 14,
              fontWeight: FontWeight.bold,
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(width: 8),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
            decoration: BoxDecoration(
              color: scheme.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(10),
            ),
            child: Text(
              '$count',
              style: TextStyle(fontSize: 11, color: scheme.onSurfaceVariant),
            ),
          ),
        ],
      ),
    );
  }
}

/// 空分区提示文本。
class _EmptySectionHint extends StatelessWidget {
  final String text;

  const _EmptySectionHint({required this.text});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 12, horizontal: 4),
      child: Text(
        text,
        style: TextStyle(fontSize: 13, color: scheme.outline),
      ),
    );
  }
}

/// 空状态：扫描中或无设备。
class _EmptyState extends StatelessWidget {
  final bool scanning;

  const _EmptyState({required this.scanning});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(
            scanning ? Icons.radar : Icons.devices_other,
            size: 72,
            color: scheme.onSurfaceVariant.withValues(alpha: 0.4),
          ),
          const SizedBox(height: 20),
          Text(
            scanning ? t.home.scanning : t.home.noDevices,
            style: TextStyle(fontSize: 16, color: scheme.onSurfaceVariant),
          ),
          const SizedBox(height: 8),
          Text(
            t.home.noDevicesHint,
            style: TextStyle(fontSize: 13, color: scheme.outline),
            textAlign: TextAlign.center,
          ),
          if (scanning) ...[
            const SizedBox(height: 16),
            SizedBox(
              width: 24,
              height: 24,
              child: CircularProgressIndicator(strokeWidth: 2, color: scheme.primary),
            ),
          ],
        ],
      ),
    );
  }
}
