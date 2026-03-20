import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';

class SettingsScreen extends ConsumerStatefulWidget {
  const SettingsScreen({super.key});

  @override
  ConsumerState<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends ConsumerState<SettingsScreen> {
  Map<String, dynamic> _config = {};
  Map<String, dynamic> _status = {};
  Map<String, dynamic> _oauthStatus = {};
  bool _loading = true;
  bool _dockerLoading = false;
  String? _dockerActionResult;

  @override
  void initState() {
    super.initState();
    _loadAll();
  }

  Future<void> _loadAll() async {
    setState(() => _loading = true);
    final api = ref.read(apiClientProvider);
    try {
      final results = await Future.wait([
        api.getConfig(),
        api.getFullStatus(),
        api.getOAuthStatus().catchError((_) => <String, dynamic>{'status': 'unknown'}),
      ]);
      setState(() {
        _config = results[0];
        _status = results[1];
        _oauthStatus = results[2];
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to load settings: $e')));
      }
    }
  }

  Future<void> _setConfig(String key, String value) async {
    try {
      await ref.read(apiClientProvider).setConfigValue(key, value);
      setState(() => _config[key] = value);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to save: $e')));
      }
    }
  }

  Widget _buildOAuthStatusChip() {
    final status = _oauthStatus['status'] ?? 'unknown';
    final expiresAt = _oauthStatus['expires_at'] as num? ?? 0;

    String label;
    Color color;
    IconData icon;

    if (status == 'valid') {
      final expiresIn = expiresAt - DateTime.now().millisecondsSinceEpoch;
      final hours = (expiresIn / 3600000).ceil();
      label = 'OAuth: valid (${hours}h)';
      color = Colors.green;
      icon = Icons.check_circle;
    } else if (status == 'expired') {
      label = 'OAuth: expired';
      color = Colors.red;
      icon = Icons.error;
    } else {
      label = 'OAuth: not configured';
      color = Colors.grey;
      icon = Icons.help_outline;
    }

    return Semantics(
      label: label,
      child: Chip(
        avatar: Icon(icon, size: 16, color: color),
        label: Text(label),
        backgroundColor: color.withValues(alpha: 0.1),
      ),
    );
  }

  Future<void> _pullImage() async {
    setState(() {
      _dockerLoading = true;
      _dockerActionResult = null;
    });
    try {
      final result = await ref.read(apiClientProvider).pullDockerImage();
      setState(() {
        _dockerLoading = false;
        _dockerActionResult = result['success'] == true
            ? 'Image pulled successfully'
            : 'Pull failed: ${result['error']}';
      });
      _loadAll();
    } catch (e) {
      setState(() {
        _dockerLoading = false;
        _dockerActionResult = 'Pull failed: $e';
      });
    }
  }

  Future<void> _buildImage() async {
    setState(() {
      _dockerLoading = true;
      _dockerActionResult = null;
    });
    try {
      final result = await ref.read(apiClientProvider).buildDockerImage();
      setState(() {
        _dockerLoading = false;
        _dockerActionResult = result['success'] == true
            ? 'Image built successfully'
            : 'Build failed: ${result['error']}';
      });
      _loadAll();
    } catch (e) {
      setState(() {
        _dockerLoading = false;
        _dockerActionResult = 'Build failed: $e';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }

    final backend = _config['execution_backend'] ?? 'local';
    final dockerAvailable = _status['docker_available'] == true;
    final sandboxReady = _status['sandbox_image_ready'] == true;
    final workerCount = _status['worker_count'] ?? 0;

    return SingleChildScrollView(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Semantics(
            header: true,
            label: 'Settings page',
            excludeSemantics: true,
            child: Text('Settings',
                style: Theme.of(context).textTheme.headlineMedium),
          ),
          const SizedBox(height: 24),

          // System Health
          _buildSection('System Health', Icons.monitor_heart, [
            _buildHealthRow('API', true),
            _buildHealthRow('Redis', _status['status'] == 'healthy'),
            _buildHealthRow('Docker', dockerAvailable),
            _buildHealthRow('Sandbox Image', sandboxReady),
            _buildInfoRow('Workers Online', '$workerCount'),
            _buildInfoRow('Execution Backend', backend),
          ]),
          const SizedBox(height: 16),

          // Execution Backend
          _buildSection('Execution Backend', Icons.settings_applications, [
            Semantics(
              label: 'Execution backend selector',
              child: SegmentedButton<String>(
                segments: const [
                  ButtonSegment(value: 'local', label: Text('Local'), icon: Icon(Icons.computer)),
                  ButtonSegment(value: 'docker', label: Text('Docker'), icon: Icon(Icons.widgets)),
                ],
                selected: {backend},
                onSelectionChanged: (sel) => _setConfig('execution_backend', sel.first),
              ),
            ),
            const SizedBox(height: 8),
            Semantics(
              label: backend == 'local'
                  ? 'Jobs run directly on the host using the local Claude CLI'
                  : 'Jobs run in isolated Docker containers with the sandbox image',
              child: Text(
                backend == 'local'
                    ? 'Jobs run directly on the host using the local Claude CLI.'
                    : 'Jobs run in isolated Docker containers with the sandbox image.',
                style: TextStyle(color: Colors.grey[400], fontSize: 13),
              ),
            ),
          ]),
          const SizedBox(height: 16),

          // Claude Authentication
          _buildSection('Claude Authentication', Icons.vpn_key, [
            // Auth status chips
            Wrap(
              spacing: 8,
              runSpacing: 4,
              children: [
                _buildOAuthStatusChip(),
                if (_config['anthropic_api_key'] == '***set***')
                  Semantics(
                    label: 'API Key: set (fallback)',
                    child: Chip(
                      avatar: const Icon(Icons.key, size: 16, color: Colors.blue),
                      label: const Text('API Key: set (fallback)'),
                      backgroundColor: Colors.blue.withValues(alpha: 0.1),
                    ),
                  ),
              ],
            ),
            const SizedBox(height: 12),

            // API Key field
            _buildEditableRow(
              'Anthropic API Key',
              _config['anthropic_api_key'] == '***set***'
                  ? ''
                  : (_config['anthropic_api_key'] ?? ''),
              (val) => _setConfig('anthropic_api_key', val),
              helperText: 'sk-ant-... Enter an API key for billed API access.',
              obscureText: true,
            ),
            const SizedBox(height: 12),
            Text(
              'Option 1: Run "claude auth login" on the server host to use your '
              'Claude subscription (Max plan). The worker auto-refreshes the token.\n'
              'Option 2: Enter an API key above for billed API access (no login needed).\n'
              'OAuth is preferred when available — API key is used as fallback.',
              style: TextStyle(color: Colors.grey[400], fontSize: 12),
            ),
          ]),
          const SizedBox(height: 16),

          // Docker / Sandbox Image
          _buildSection('Sandbox Image', Icons.inventory_2, [
            _buildEditableRow(
              'Image Name',
              _config['sandbox_image'] ?? 'claw-sandbox:latest',
              (val) => _setConfig('sandbox_image', val),
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Semantics(
                  label: 'Docker status indicator',
                  child: Chip(
                    avatar: Icon(
                      dockerAvailable ? Icons.check_circle : Icons.error,
                      size: 16,
                      color: dockerAvailable ? Colors.green : Colors.red,
                    ),
                    label: Text(dockerAvailable ? 'Docker Available' : 'Docker Not Found'),
                  ),
                ),
                const SizedBox(width: 8),
                Semantics(
                  label: 'Sandbox image status',
                  child: Chip(
                    avatar: Icon(
                      sandboxReady ? Icons.check_circle : Icons.warning,
                      size: 16,
                      color: sandboxReady ? Colors.green : Colors.orange,
                    ),
                    label: Text(sandboxReady ? 'Image Ready' : 'Image Not Found'),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                FilledButton.icon(
                  onPressed: _dockerLoading || !dockerAvailable ? null : _pullImage,
                  icon: const Icon(Icons.download),
                  label: const Text('Pull Image'),
                ),
                const SizedBox(width: 8),
                FilledButton.tonalIcon(
                  onPressed: _dockerLoading || !dockerAvailable ? null : _buildImage,
                  icon: const Icon(Icons.build),
                  label: const Text('Build Image'),
                ),
                if (_dockerLoading) ...[
                  const SizedBox(width: 12),
                  const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2)),
                ],
              ],
            ),
            if (_dockerActionResult != null) ...[
              const SizedBox(height: 8),
              Semantics(
                label: _dockerActionResult!,
                child: Text(
                  _dockerActionResult!,
                  style: TextStyle(
                    color: _dockerActionResult!.contains('failed') || _dockerActionResult!.contains('Failed')
                        ? Colors.red
                        : Colors.green,
                    fontSize: 13,
                  ),
                ),
              ),
            ],
          ]),
          const SizedBox(height: 16),

          // Resource Limits
          _buildSection('Default Resource Limits', Icons.memory, [
            _buildEditableRow(
              'Memory Limit',
              _config['docker_memory_limit'] ?? '4g',
              (val) => _setConfig('docker_memory_limit', val),
              helperText: 'e.g. 4g, 512m',
            ),
            const SizedBox(height: 8),
            _buildEditableRow(
              'CPU Limit',
              _config['docker_cpu_limit'] ?? '2.0',
              (val) => _setConfig('docker_cpu_limit', val),
              helperText: 'Number of CPUs (e.g. 2.0)',
            ),
            const SizedBox(height: 8),
            _buildEditableRow(
              'PID Limit',
              _config['docker_pids_limit'] ?? '256',
              (val) => _setConfig('docker_pids_limit', val),
              helperText: 'Max processes per container',
            ),
          ]),
          const SizedBox(height: 16),

          // Credential Mounts
          _buildCredentialMountsSection(),
          const SizedBox(height: 16),

          // About
          _buildSection('About', Icons.info_outline, [
            Semantics(
              label: 'ClaudeCodeClaw version',
              child: const Text('ClaudeCodeClaw v0.1.0'),
            ),
            const SizedBox(height: 4),
            Text('Job queue orchestrator for Claude Code',
                style: TextStyle(color: Colors.grey[400])),
          ]),
        ],
      ),
    );
  }

  Widget _buildSection(String title, IconData icon, List<Widget> children) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(icon, size: 20),
                const SizedBox(width: 8),
                Semantics(
                  header: true,
                  label: '$title section',
                  excludeSemantics: true,
                  child: Text(title,
                      style: const TextStyle(
                          fontWeight: FontWeight.bold, fontSize: 16)),
                ),
              ],
            ),
            const SizedBox(height: 12),
            ...children,
          ],
        ),
      ),
    );
  }

  Widget _buildHealthRow(String label, bool healthy) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          Icon(
            healthy ? Icons.check_circle : Icons.error,
            size: 16,
            color: healthy ? Colors.green : Colors.red,
          ),
          const SizedBox(width: 8),
          Semantics(
            label: '$label ${healthy ? "healthy" : "unhealthy"}',
            excludeSemantics: true,
            child: Text(label),
          ),
        ],
      ),
    );
  }

  Widget _buildInfoRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          const Icon(Icons.info_outline, size: 16, color: Colors.grey),
          const SizedBox(width: 8),
          Semantics(
            label: '$label: $value',
            excludeSemantics: true,
            child: Text('$label: $value'),
          ),
        ],
      ),
    );
  }

  Widget _buildEditableRow(
    String label,
    String currentValue,
    Function(String) onSave, {
    String? helperText,
    bool obscureText = false,
  }) {
    return Row(
      children: [
        SizedBox(
          width: 300,
          child: TextFormField(
            initialValue: currentValue,
            obscureText: obscureText,
            decoration: InputDecoration(
              labelText: label,
              helperText: helperText,
              isDense: true,
            ),
            style: const TextStyle(fontFamily: 'monospace', fontSize: 14),
            onFieldSubmitted: onSave,
          ),
        ),
      ],
    );
  }

  Widget _buildCredentialMountsSection() {
    List<dynamic> mounts = [];
    try {
      final raw = _config['docker_credential_mounts'];
      if (raw is String) {
        mounts = jsonDecode(raw) as List;
      } else if (raw is List) {
        mounts = raw;
      }
    } catch (_) {}

    return _buildSection('Credential Mounts', Icons.vpn_key, [
      Text(
        'Host paths mounted into Docker containers for authentication.',
        style: TextStyle(color: Colors.grey[400], fontSize: 13),
      ),
      const SizedBox(height: 8),
      ...mounts.asMap().entries.map((entry) {
        final i = entry.key;
        final m = Map<String, dynamic>.from(entry.value);
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Row(
            children: [
              Expanded(
                child: Semantics(
                  label: 'Mount ${m['host_path']} to ${m['container_path']}',
                  child: Text(
                    '${m['host_path']} → ${m['container_path']}${m['readonly'] == true ? ' (ro)' : ' (rw)'}',
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                  ),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.delete, size: 18),
                onPressed: () => _removeMount(i, mounts),
                tooltip: 'Remove mount',
              ),
            ],
          ),
        );
      }),
      const SizedBox(height: 8),
      TextButton.icon(
        onPressed: () => _showAddMountDialog(mounts),
        icon: const Icon(Icons.add, size: 18),
        label: const Text('Add Mount'),
      ),
    ]);
  }

  Future<void> _removeMount(int index, List<dynamic> mounts) async {
    final updated = List<dynamic>.from(mounts)..removeAt(index);
    await _setConfig('docker_credential_mounts', jsonEncode(updated));
  }

  Future<void> _showAddMountDialog(List<dynamic> mounts) async {
    final hostCtrl = TextEditingController();
    final containerCtrl = TextEditingController();
    bool readonly = true;

    final result = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('Add Credential Mount'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: hostCtrl,
                decoration: const InputDecoration(
                  labelText: 'Host Path',
                  hintText: '~/.ssh',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: containerCtrl,
                decoration: const InputDecoration(
                  labelText: 'Container Path',
                  hintText: '/home/claw/.ssh',
                ),
              ),
              const SizedBox(height: 8),
              SwitchListTile(
                title: const Text('Read-only'),
                value: readonly,
                onChanged: (v) => setDialogState(() => readonly = v),
              ),
            ],
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx, false),
                child: const Text('Cancel')),
            FilledButton(
                onPressed: () => Navigator.pop(ctx, true),
                child: const Text('Add')),
          ],
        ),
      ),
    );

    if (result != true) return;
    if (hostCtrl.text.trim().isEmpty || containerCtrl.text.trim().isEmpty) return;

    final updated = List<dynamic>.from(mounts)
      ..add({
        'host_path': hostCtrl.text.trim(),
        'container_path': containerCtrl.text.trim(),
        'readonly': readonly,
      });
    await _setConfig('docker_credential_mounts', jsonEncode(updated));
  }
}
