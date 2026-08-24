import 'dart:async';
import 'dart:io' show Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/pages/history_drawer.dart';
import 'package:hyx_app/pages/log_sheet.dart';

import 'package:hyx_app/pages/transfer_progress_sheet.dart';
import 'package:hyx_app/provider/device_provider.dart';
import 'package:hyx_app/provider/log_provider.dart';
import 'package:hyx_app/provider/transfer_provider.dart';
import 'package:hyx_app/util/update_checker.dart';
import 'package:hyx_app/widget/device_card.dart';
import 'package:hyx_isolates/rust/api/model.dart' as model;
import 'package:package_info_plus/package_info_plus.dart';
import 'package:refena_flutter/refena_flutter.dart';
import 'package:share_handler/share_handler.dart';

/// 与 MainActivity.kt 通信的 MethodChannel，用于通知 Dart 侧已就绪、
/// 可重放启动期间被暂存的分享 intent。
const _channel = MethodChannel('com.ojbkxc.hyx_app/hyx');

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

  /// share_handler 的 sharedMediaStream 订阅，dispose 时取消。
  StreamSubscription<SharedMedia>? _sharedMediaSubscription;

  /// 通过分享 intent 收到、等待用户选择目标设备的文件路径列表。
  /// 非 null 表示有待处理分享；由 [_handleSharedMedia] 写入，
  /// [_showDevicePickerForShare] 消费后置 null。
  List<String>? _pendingShareFiles;

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
      // 初始化分享处理（Android）。
      _initShareHandler();
    });
  }

  @override
  void dispose() {
    unawaited(_sharedMediaSubscription?.cancel());
    _sharedMediaSubscription = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {

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

    // 收到分享文件后，等帧渲染完再弹设备选择对话框（避免在 build 中直接 showDialog）。
    if (_pendingShareFiles != null && _pendingShareFiles!.isNotEmpty) {
      final files = _pendingShareFiles;
      _pendingShareFiles = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        _showDevicePickerForShare(files!);
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

    );
  }

  /// 侧边栏 Drawer：本设备信息 + 历史记录 + 设置。
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
                    content: Text(t.devices.deleted(name: d.name)),
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
      StartSendAction(
        peerAddress: device.addr,
        // 有缓存指纹 → 直连 pin 跳过发现；空串（尚未 TOFU 过）→ null 走发现/TOFU 回退。
        cachedFingerprint: device.fingerprint.isNotEmpty ? device.fingerprint : null,
        filePath: path,
      ),
    ));
    _showTransferSheet();
  }

  /// 初始化 share_handler：获取启动时带来的分享、订阅后续分享流，
  /// 并通知 Android 侧重放启动期间被暂存的 SEND intent。
  ///
  /// 仅在 Android 启用：share_handler 的 initial media / stream 在桌面端
  /// 无意义，且 `shareIntentReady` MethodChannel 仅在 MainActivity 实现。
  void _initShareHandler() {
    if (!Platform.isAndroid) return;

    final shareHandler = ShareHandlerPlatform.instance;

    // 启动时若由分享 intent 拉起，先取出并处理。
    unawaited(shareHandler.getInitialSharedMedia().then((payload) {
      if (payload != null) {
        _handleSharedMedia(payload);
      }
    }));

    // 订阅后续分享事件（应用已在前台时再次被分享）。
    unawaited(_sharedMediaSubscription?.cancel());
    _sharedMediaSubscription = shareHandler.sharedMediaStream.listen((payload) {
      _handleSharedMedia(payload);
    });

    // 通知 MainActivity：Dart 侧已订阅 sharedMediaStream，
    // 可重放 onNewIntent 期间被暂存的 SEND intent。
    // 两条消息走同一 messenger 且有序：先 attach stream，再 flush，
    // 保证重放的 intent 能被 stream 捕获。
    unawaited(_channel.invokeMethod('shareIntentReady'));
  }

  /// 从 [SharedMedia] 中提取文件路径，写入 [_pendingShareFiles]，
  /// 由 build 中的 postFrameCallback 触发设备选择对话框。
  void _handleSharedMedia(SharedMedia payload) {
    final paths = <String>[];
    final attachments = payload.attachments;
    if (attachments != null) {
      for (final a in attachments) {
        if (a == null) continue;
        // share_handler 的 SharedAttachment.path 在 Android 上是 content:// 或文件路径，
        // 可能为 null（官方 API 将其声明为可空），需显式判空。
        final p = a.path;
        if (p != null && p.isNotEmpty) {
          paths.add(p);
        }
      }
    }
    if (paths.isEmpty) return;
    _pendingShareFiles = paths;
    // 触发一次 setState，确保 build 中的 postFrameCallback 被调度。
    if (mounted) {
      setState(() {});
    }
  }

  /// 弹出设备选择对话框，让用户挑选分享文件的目标设备。
  void _showDevicePickerForShare(List<String> files) {
    final devState = context.read(deviceProvider);
    final onlineDevices = devState.onlineDevices;

    if (onlineDevices.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: const Text('附近没有在线设备，无法发送'),
          duration: const Duration(seconds: 3),
        ),
      );
      return;
    }

    showDialog<void>(
      context: context,
      builder: (ctx) {
        return AlertDialog(
          title: const Text('选择发送目标'),
          content: SizedBox(
            width: double.maxFinite,
            child: ListView.builder(
              shrinkWrap: true,
              itemCount: onlineDevices.length,
              itemBuilder: (listCtx, i) {
                final d = onlineDevices[i];
                return ListTile(
                  leading: CircleAvatar(
                    backgroundColor: Theme.of(listCtx).colorScheme.primaryContainer,
                    child: Text(
                      d.name.isNotEmpty ? d.name[0].toUpperCase() : '?',
                      style: TextStyle(color: Theme.of(listCtx).colorScheme.onPrimaryContainer),
                    ),
                  ),
                  title: Text(d.name),
                  onTap: () {
                    Navigator.pop(listCtx);
                    _sendSharedFilesToPeer(d, files);
                  },
                );
              },
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx),
              child: Text(t.history.cancel),
            ),
          ],
        );
      },
    );
  }

  /// 把分享得到的文件发送到选定设备。
  ///
  /// 当前 [StartSendAction] 一次只发一个文件，且 busy 时会拒绝新传输，
  /// 因此多文件场景下仅发送第一个，其余通过 SnackBar 提示。
  /// 后续可扩展为串行队列。
  void _sendSharedFilesToPeer(KnownDevice device, List<String> files) {
    if (files.isEmpty) return;
    final first = files.first;

    unawaited(context.redux(transferProvider).dispatchAsync(
      StartSendAction(
        peerAddress: device.addr,
        // 有缓存指纹 → 直连 pin 跳过发现；空串（尚未 TOFU 过）→ null 走发现/TOFU 回退。
        cachedFingerprint: device.fingerprint.isNotEmpty ? device.fingerprint : null,
        filePath: first,
      ),
    ));
    _showTransferSheet();

    if (files.length > 1) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('共 ${files.length} 个文件，当前仅发送第一个（${files.first.split(RegExp(r'[/\\]')).last}）'),
          duration: const Duration(seconds: 3),
        ),
      );
    }
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
        content: Text(t.devices.deleteConfirm(name: name)),
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
  /// 当前版本号通过 package_info_plus 动态获取，与打包版本一致。
  Future<void> _checkForUpdate() async {
    try {
      final packageInfo = await PackageInfo.fromPlatform();
      final info = await UpdateChecker.check(packageInfo.version);
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
