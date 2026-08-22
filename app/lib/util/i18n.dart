import 'dart:ui' as ui;

import 'package:hyx_app/gen/strings.g.dart';
import 'package:logging/logging.dart';

final _logger = Logger('i18n');

/// 初始化 i18n（slang）。
///
/// 参照 LocalSend `util/i18n.dart` 的 `initI18n` 但简化：根据系统 locale
/// 设置 `TranslationProvider` 的 locale，不做持久化覆盖。
/// `strings.g.dart` 由 `slang_build_runner` 从 `assets/i18n/*.json` 生成。
Future<void> initI18n() async {
  try {
    // 取系统 locale 的语言代码，匹配 slang 编译期已知的 locale。
    final systemLang = ui.PlatformDispatcher.instance.locale.languageCode;
    await LocaleSettings.setLocaleRaw(systemLang);
  } catch (e) {
    _logger.warning('i18n init failed: $e');
  }
}
