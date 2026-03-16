import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';

class SettingsScreen extends ConsumerStatefulWidget {
  const SettingsScreen({super.key});

  @override
  ConsumerState<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends ConsumerState<SettingsScreen> {
  bool _connected = false;
  bool _checking = true;

  @override
  void initState() {
    super.initState();
    _checkConnection();
  }

  Future<void> _checkConnection() async {
    setState(() => _checking = true);
    try {
      await ref.read(apiClientProvider).getStatus();
      setState(() {
        _connected = true;
        _checking = false;
      });
    } catch (_) {
      setState(() {
        _connected = false;
        _checking = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Semantics(header: true, label: 'Settings page', excludeSemantics: true, child: Text('Settings', style: Theme.of(context).textTheme.headlineMedium)),
          const SizedBox(height: 24),

          // Connection status
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                children: [
                  Icon(
                    _checking
                        ? Icons.sync
                        : _connected
                            ? Icons.check_circle
                            : Icons.error,
                    color: _checking
                        ? Colors.orange
                        : _connected
                            ? Colors.green
                            : Colors.red,
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Semantics(label: 'API Connection status', excludeSemantics: true, child: const Text('API Connection',
                            style: TextStyle(fontWeight: FontWeight.bold))),
                        Semantics(label: _checking ? 'Connection checking' : _connected ? 'Connection status connected' : 'Connection status disconnected', excludeSemantics: true, child: Text(
                          _checking
                              ? 'Checking...'
                              : _connected
                                  ? 'Connected to http://localhost:8080'
                                  : 'Not connected',
                          style: TextStyle(
                            color: _connected ? Colors.green : Colors.red,
                          ),
                        )),
                      ],
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.refresh),
                    onPressed: _checkConnection,
                    tooltip: 'Check connection',
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 16),

          // Info
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text('About',
                      style: TextStyle(fontWeight: FontWeight.bold)),
                  const SizedBox(height: 8),
                  const Text('ClaudeCodeClaw v0.1.0'),
                  const SizedBox(height: 4),
                  Text('Job queue orchestrator for Claude Code',
                      style: TextStyle(color: Colors.grey[400])),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
