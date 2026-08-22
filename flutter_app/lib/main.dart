import 'package:flutter/material.dart';

import 'src/rust/api/discovery.dart';
import 'src/rust/api/identity.dart';
import 'src/rust/frb_generated.dart';

void main() => runApp(const HyxApp());

class HyxApp extends StatelessWidget {
  const HyxApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'HyX',
      theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
      home: const HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  String? _deviceId;
  String? _fingerprint;
  List<Peer> _peers = const [];
  bool _scanning = false;

  @override
  void initState() {
    super.initState();
    _bootstrap();
  }

  Future<void> _bootstrap() async {
    await RustLib.init();
    // createDevice / selfDeviceId are #[frb(sync)]: synchronous Dart.
    final id = selfDeviceId();
    final fp = createDevice();
    setState(() {
      _deviceId = id;
      _fingerprint = fp;
    });
  }

  Future<void> _scan() async {
    setState(() => _scanning = true);
    final peers = await discoverPeers();
    setState(() {
      _peers = peers;
      _scanning = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('HyX — LAN 设备')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('device_id: ${_deviceId ?? '…'}'),
          Text('fingerprint: ${_fingerprint ?? '…'}'),
          const SizedBox(height: 16),
          FilledButton.icon(
            onPressed: _scanning ? null : _scan,
            icon: const Icon(Icons.wifi_tethering),
            label: Text(_scanning ? '扫描中…' : '扫描局域网设备'),
          ),
          const SizedBox(height: 16),
          for (final p in _peers)
            Card(
              child: ListTile(
                leading: const Icon(Icons.devices),
                title: Text(p.deviceName),
                subtitle: Text('${p.address}\n${p.deviceId}'),
              ),
            ),
          if (_peers.isEmpty && !_scanning)
            const Center(child: Padding(
              padding: EdgeInsets.all(24),
              child: Text('未发现设备'),
            )),
        ],
      ),
    );
  }
}