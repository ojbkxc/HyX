import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:hyx_app/provider/log_provider.dart';
import 'package:hyx_app/util/i18n.dart';
import 'package:hyx_isolates/rust/api/logging.dart' as rust_logging;
import 'package:hyx_isolates/rust/frb_generated.dart';
import 'package:logging/logging.dart';
import 'package:refena_flutter/refena_flutter.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:window_manager/window_manager.dart';

final _logger = Logger('Init');

/// 初始化 [Logger] 根记录器层级。
///
/// 设置 [Logger.root] 的 [hierarchicalLoggingEnabled] 并将根级别设为 [level]。
void initLogger(Level level) {
  hierarchicalLoggingEnabled = true;
  Logger.root.level = level;
}

/// 在 `MaterialApp` 启动前执行的初始化。
///
/// 参照 LocalSend `preInit` 但大幅简化：仅保留
/// 1. `WidgetsFlutterBinding` 初始化；
/// 2. 日志初始化；
/// 3. `RustLib.init()`（flutter_rust_bridge 生成的 FFI 绑定）；
/// 4. debug 模式下启用 Rust debug 日志；
/// 5. i18n 初始化；
/// 6. 桌面端窗口管理器初始化；
/// 7. 构造 `RefenaContainer` 并预热日志回调（捕获 Rust 侧 tracing 事件）。
///
/// 去掉了 LocalSend 的 persistence service、isolate container、tray、
/// autostart、context menu、share handler、purchase 等——这些可在后续任务中
/// 按需引入。
///
/// 返回的 [RefenaContainer] 由 [main.dart] 挂载到 `RefenaScope`。
Future<RefenaContainer> preInit(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();

  initLogger(args.contains('-v') || args.contains('--verbose') ? Level.ALL : Level.INFO);

  // flutter_rust_bridge 生成的 FFI 绑定初始化。
  await RustLib.init();

  if (kDebugMode) {
    try {
      await rust_logging.enableDebugLogging();
    } catch (e) {
      _logger.warning('Enabling debug logging failed', e);
    }
  }

  await initI18n();

  // 桌面端窗口管理器初始化（移动端无操作）。
  if (defaultTargetPlatform == TargetPlatform.windows ||
      defaultTargetPlatform == TargetPlatform.macOS ||
      defaultTargetPlatform == TargetPlatform.linux) {
    try {
      await WindowManager.instance.ensureInitialized();
    } catch (e) {
      _logger.warning('Window manager init failed: $e');
    }
  }

  // 触发 SharedPreferences 初始化，确保持久化可用。
  // 后续 provider 会通过 ref 读取；此处仅预热。
  try {
    await SharedPreferences.getInstance();
  } catch (e) {
    _logger.warning('SharedPreferences init failed: $e');
  }

  final container = RefenaContainer(
    observers: kDebugMode ? [CustomRefenaObserver()] : [],
    platformHint: RefenaScope.getPlatformHint(),
  );

  // 预热日志回调：在 widget 树挂载前通过 container 直接 dispatch，
  // 这样 Rust 侧 tracing 事件从启动伊始就被收集到 logProvider。
  try {
    unawaited(container.redux(logProvider).dispatchAsync(InstallLogCallbackAction()));
  } catch (e) {
    _logger.warning('Log callback install failed: $e');
  }

  return container;
}

/// 简易 Refena 观察者，debug 模式下打印状态变化。
class CustomRefenaObserver extends RefenaObserver {
  @override
  void handleEvent(RefenaEvent event) {
    if (kDebugMode) {
      _logger.fine(event.toString());
    }
  }
}
