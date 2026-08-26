import 'dart:async';

import 'package:flutter/material.dart';
import 'package:hyx_isolates/rust/api/device.dart' as rust_device;
import 'package:shared_preferences/shared_preferences.dart';

/// SharedPreferences 中持久化自定义设备名称的 key。
///
/// 与 home_page.dart 启动时读取逻辑保持一致，改一处两边都要同步。
const _kCustomNamePrefKey = 'hyx_custom_device_name';

/// 设置页面：让用户自定义本设备名称。
///
/// 名称会通过 [rust_device.setDeviceName] 同步到 Rust 侧，
/// 之后 beacon 广播会携带新名称，peer 收到后自动显示。
///
/// 持久化用 SharedPreferences，key 为 [kCustomNamePrefKey]。
/// 空串视为重置为默认名（Rust 侧会回退到 `hyx-{id前6位}`）。
///
/// 样式与 home_page.dart 保持一致：使用 colorScheme、Material 3 风格。
class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key});

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  final _controller = TextEditingController();

  /// 是否正在保存（防止重复点击）。
  bool _saving = false;

  /// 输入框初始值是否已加载完成，避免闪一下空串再被预填覆盖。
  bool _loaded = false;

  @override
  void initState() {
    super.initState();
    unawaited(_loadName());
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  /// 从 SharedPreferences 读取已保存的自定义名称，预填到输入框。
  Future<void> _loadName() async {
    final prefs = await SharedPreferences.getInstance();
    final saved = prefs.getString(_kCustomNamePrefKey) ?? '';
    if (!mounted) return;
    _controller.text = saved;
    setState(() => _loaded = true);
  }

  /// 保存当前输入的名称：
  /// 1. trim 后调 [rust_device.setDeviceName] 同步到 Rust 侧；
  /// 2. 写入 SharedPreferences 持久化；
  /// 3. 返回上一页。
  ///
  /// 空串视为重置为默认名（Rust 侧逻辑）。
  Future<void> _save() async {
    if (_saving) return;
    setState(() => _saving = true);
    try {
      final name = _controller.text.trim();
      // 同步到 Rust 侧（fire-and-forget，setDeviceName 是同步函数）。
      unawaited(rust_device.setDeviceName(name: name));
      // 持久化。
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(_kCustomNamePrefKey, name);
      if (!mounted) return;
      Navigator.pop(context);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;

    return Scaffold(
      appBar: AppBar(
        title: const Text('设置'),
      ),
      body: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // 区块标题：设备名称。
            Padding(
              padding: const EdgeInsets.fromLTRB(4, 12, 4, 8),
              child: Text(
                '设备名称',
                style: TextStyle(
                  fontSize: 14,
                  fontWeight: FontWeight.bold,
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ),
            // 输入框。
            TextField(
              controller: _controller,
              decoration: InputDecoration(
                hintText: '输入自定义设备名称',
                prefixIcon: const Icon(Icons.badge_outlined),
                border: const OutlineInputBorder(),
                enabled: _loaded,
              ),
              textInputAction: TextInputAction.done,
              maxLength: 32,
              onSubmitted: (_) => _save(),
            ),
            const SizedBox(height: 12),
            // 说明文字。
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Text(
                '此名称将显示在其它设备的设备列表中。留空则使用默认名称。',
                style: TextStyle(fontSize: 13, color: scheme.outline),
              ),
            ),
            const Spacer(),
            // 保存按钮。
            SizedBox(
              width: double.infinity,
              child: FilledButton.icon(
                onPressed: _saving || !_loaded ? null : _save,
                icon: _saving
                    ? SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: scheme.onPrimary,
                        ),
                      )
                    : const Icon(Icons.save),
                label: Text(_saving ? '保存中…' : '保存'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}