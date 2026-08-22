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
/// 简化设计（比 LocalSend 更简单）：
/// - **无 tab 切换** — 主页就是设备列表，打开应用直接看到附近设备。
/// - **无配对码/二维码主入口** — 局域网自动发现是唯一主流程。配对码/二维码
///   只在跨 NAT 时作为备选（侧边栏 Drawer 入口）。
/// - **自动接收** — allowTransfer=true 的设备自动接收文件，不弹确认框。
/// - **拖拽发送** — 桌面端支持拖拽文件到设备卡片上发送。
/// - **传输进度浮层** — BottomSheet 显示进度。
/// - **历史记录在侧边栏 Drawer** — 不占主页面。
class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> with Refena {
  @override
  void initState() {
    super.initState();
    ensureRef((ref) async {
      // 注册日志回调。
      await ref.redux(logProvider).dispatchAsync(InstallLogCallbackAction());
      // 加载本设备身份（fire-and-forget）。
      unawaited(ref.redux(deviceProvider).dispatchAsync(LoadMyDeviceAction()));
      // 启动自动发现。
      ref.redux(deviceProvider).dispatch(StartDiscoveryAction());
      // 检测更新（fire-and-forget）
      unawaited(_checkForUpdate());
    });
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final devState = context.watch(deviceProvider);
    final transferState = context.watch(transferProvider);

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
      floatingActionButton: transferState.busy
          ? null
          : FloatingActionButton.extended(
              onPressed: () => _startReceive(context),
              icon: const Icon(Icons.download),
              label: Text(t.home.startReceive),
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

  /// 主内容：设备列表或空状态。
  Widget _buildBody(BuildContext context, DeviceState devState) {
    final peers = devState.peers;

    if (peers.isEmpty) {
      return _EmptyState(scanning: devState.scanning);
    }

    return ListView.builder(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 96),
      itemCount: peers.length,
      itemBuilder: (_, i) {
        final peer = peers[i];
        return Padding(
          padding: const EdgeInsets.only(bottom: 10),
          child: DeviceCard(
            peer: peer,
            onTap: () => _sendToPeer(context, peer),
          ),
        );
      },
    );
  }

  /// 点击设备 → 选择文件 → 发送。
  Future<void> _sendToPeer(BuildContext context, model.RsDiscoveredPeer peer) async {
    final result = await FilePicker.pickFiles(allowMultiple: false);
    if (result == null || result.files.isEmpty) return;
    final path = result.files.first.path;
    if (path == null) return;

    if (!context.mounted) return;
    unawaited(context.redux(transferProvider).dispatchAsync(
      StartSendAction(peerAddress: peer.addr, filePath: path),
    ));
    showTransferProgressSheet(context);
  }

  /// FAB：启动接收监听。
  void _startReceive(BuildContext context) {
    unawaited(context.redux(transferProvider).dispatchAsync(StartReceiveAction()));
    showTransferProgressSheet(context);
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
