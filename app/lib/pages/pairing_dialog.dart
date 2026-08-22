import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:hyx_app/gen/strings.g.dart';
import 'package:hyx_app/pages/transfer_progress_sheet.dart';
import 'package:hyx_app/provider/transfer_provider.dart';
import 'package:refena_flutter/refena_flutter.dart';

/// 配对码输入对话框。
///
/// 从 Drawer 入口打开（跨 NAT 备选流程）。用户输入配对码 + 服务器地址，
/// 选择角色（发送/接收）：
/// - 发送：选择文件 → `pairSend`
/// - 接收：直接 `pairRendezvous`
///
/// 对应 Kotlin `TransferScreen` 中 `showEnterDialog` + `onEnterCode` 的逻辑，
/// 但独立成对话框，因为 HyX 把配对码作为侧边栏备选入口而非主流程。
Future<void> showPairingDialog(BuildContext context) async {
  await showDialog(
    context: context,
    builder: (_) => const PairingDialog(),
  );
}

class PairingDialog extends StatefulWidget {
  const PairingDialog({super.key});

  @override
  State<PairingDialog> createState() => _PairingDialogState();
}

class _PairingDialogState extends State<PairingDialog> with Refena {
  final _codeController = TextEditingController();
  final _serverController = TextEditingController(text: 'rendezvous.hyx.dev');
  final _portController = TextEditingController(text: '14570');
  bool _isSend = true;
  String? _filePath;

  @override
  void dispose() {
    _codeController.dispose();
    _serverController.dispose();
    _portController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return AlertDialog(
      title: Text(t.pair.title),
      content: SizedBox(
        width: double.maxFinite,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              // 角色 segmented。
              SegmentedButton<bool>(
                segments: [
                  ButtonSegment(value: true, label: Text(t.pair.send)),
                  ButtonSegment(value: false, label: Text(t.pair.receive)),
                ],
                selected: {_isSend},
                onSelectionChanged: (s) => setState(() => _isSend = s.first),
              ),
              const SizedBox(height: 16),
              // 配对码。
              TextField(
                controller: _codeController,
                decoration: InputDecoration(
                  labelText: t.pair.code,
                  hintText: t.pair.codeHint,
                  counterText: '',
                ),
                maxLength: 8,
                textCapitalization: TextCapitalization.characters,
                onChanged: (v) => _codeController.value = TextEditingValue(
                  text: v.toUpperCase(),
                  selection: TextSelection.collapsed(offset: v.length),
                ),
              ),
              const SizedBox(height: 12),
              // 服务器地址。
              TextField(
                controller: _serverController,
                decoration: InputDecoration(labelText: t.pair.server),
              ),
              const SizedBox(height: 12),
              // 端口。
              TextField(
                controller: _portController,
                decoration: InputDecoration(labelText: t.pair.port),
                keyboardType: TextInputType.number,
              ),
              const SizedBox(height: 12),
              // 发送方：文件选择。
              if (_isSend) ...[
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        _filePath != null
                            ? _filePath!.split(RegExp(r'[/\\]')).last
                            : t.pair.noFile,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: _filePath != null ? scheme.onSurface : scheme.onSurfaceVariant),
                      ),
                    ),
                    TextButton(
                      onPressed: _pickFile,
                      child: Text(t.pair.selectFile),
                    ),
                  ],
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t.pair.cancel)),
        FilledButton(
          onPressed: _start,
          child: Text(t.pair.start),
        ),
      ],
    );
  }

  Future<void> _pickFile() async {
    final result = await FilePicker.platform.pickFiles();
    if (result != null && result.files.isNotEmpty) {
      setState(() => _filePath = result.files.first.path);
    }
  }

  void _start() {
    final code = _codeController.text.trim();
    final server = _serverController.text.trim();
    final port = int.tryParse(_portController.text.trim()) ?? 0;
    if (code.isEmpty || server.isEmpty) return;
    if (_isSend && _filePath == null) return;

    Navigator.pop(context);
    if (_isSend) {
      unawaited(context.redux(transferProvider).dispatch(
        StartPairSendAction(code: code, server: server, filePath: _filePath!, port: port),
      ));
    } else {
      unawaited(context.redux(transferProvider).dispatch(
        StartPairReceiveAction(code: code, server: server, port: port),
      ));
    }
    showTransferProgressSheet(context);
  }
}