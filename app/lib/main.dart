import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:hyx_app/config/init.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/pages/home_page.dart';
import 'package:refena_flutter/refena_flutter.dart';

/// HyX 应用入口。
///
/// 参照 LocalSend `main.dart` 但简化：去掉 tray/window watcher、share handler、
/// WebRTC、purchase 等，仅保留 Refena 状态管理 + i18n + 基础 MaterialApp。
Future<void> main(List<String> args) async {
  final RefenaContainer container;
  try {
    container = await preInit(args);
  } catch (e, stackTrace) {
    // 初始化失败时展示错误界面，避免黑屏。
    runApp(_InitErrorApp(error: e, stackTrace: stackTrace));
    return;
  }

  runApp(
    RefenaScope.withContainer(
      container: container,
      child: TranslationProvider(
        child: const HyXApp(),
      ),
    ),
  );
}

/// HyX 根 Widget。
///
/// 对应 LocalSend `LocalSendApp`，但去掉了 TrayWatcher / WindowWatcher /
/// LifeCycleWatcher / ShortcutWatcher 等桌面端 watcher，仅保留 MaterialApp +
/// i18n + 主题。后续按需在 [HomePage] 内部按平台挂载 watcher。
class HyXApp extends StatelessWidget {
  const HyXApp();

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: t.appName,
      locale: TranslationProvider.of(context).flutterLocale,
      supportedLocales: AppLocaleUtils.supportedLocales,
      localizationsDelegates: GlobalMaterialLocalizations.delegates,
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorSchemeSeed: const Color(0xFF1565C0),
        useMaterial3: true,
      ),
      darkTheme: ThemeData(
        colorSchemeSeed: const Color(0xFF42A5F5),
        useMaterial3: true,
        brightness: Brightness.dark,
      ),
      home: const HomePage(),
    );
  }
}

/// 初始化失败时展示的简易错误界面。
class _InitErrorApp extends StatelessWidget {
  final Object error;
  final StackTrace stackTrace;

  const _InitErrorApp({
    required this.error,
    required this.stackTrace,
  });

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: Center(
          child: Padding(
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                const Icon(Icons.error_outline, size: 64, color: Colors.red),
                const SizedBox(height: 16),
                const Text(
                  'HyX 初始化失败',
                  style: TextStyle(fontSize: 20, fontWeight: FontWeight.bold),
                ),
                const SizedBox(height: 8),
                Text(
                  error.toString(),
                  textAlign: TextAlign.center,
                  style: const TextStyle(fontSize: 14),
                ),
                const SizedBox(height: 16),
                Text(
                  stackTrace.toString(),
                  textAlign: TextAlign.left,
                  style: const TextStyle(fontSize: 10, fontFamily: 'monospace'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}