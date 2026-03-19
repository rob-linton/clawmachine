import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';
import '../models/credential.dart';

class CredentialsScreen extends ConsumerStatefulWidget {
  const CredentialsScreen({super.key});

  @override
  ConsumerState<CredentialsScreen> createState() => _CredentialsScreenState();
}

class _CredentialsScreenState extends ConsumerState<CredentialsScreen> {
  List<Credential> _credentials = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final creds = await ref.read(apiClientProvider).listCredentials();
      setState(() {
        _credentials = creds;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _delete(String id) async {
    final confirm = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Credential'),
        content: const Text(
            'Are you sure? Workspaces bound to this credential will lose access.'),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Delete')),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).deleteCredential(id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _showCreateEditDialog({Credential? existing}) async {
    final idCtrl = TextEditingController(text: existing?.id ?? '');
    final nameCtrl = TextEditingController(text: existing?.name ?? '');
    final descCtrl = TextEditingController(text: existing?.description ?? '');
    // For editing, show existing keys as placeholders
    final valuesCtrl = TextEditingController(
      text: existing != null
          ? existing.keys.map((k) => '$k=').join('\n')
          : '',
    );

    final isEdit = existing != null;

    await showDialog(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Semantics(
          header: true,
          label: isEdit ? 'Edit Credential' : 'Create Credential',
          child: Text(isEdit ? 'Edit Credential' : 'Create Credential'),
        ),
        content: SizedBox(
          width: 500,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (!isEdit)
                  TextField(
                    controller: idCtrl,
                    decoration: const InputDecoration(
                        labelText: 'ID (e.g., prod-aws)',
                        border: OutlineInputBorder()),
                  ),
                if (!isEdit) const SizedBox(height: 12),
                TextField(
                  controller: nameCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Name (e.g., Production AWS)',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: descCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Description',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: valuesCtrl,
                  maxLines: 6,
                  decoration: const InputDecoration(
                    labelText: 'Key=Value pairs (one per line)',
                    hintText:
                        'AWS_ACCESS_KEY_ID=AKIA...\nAWS_SECRET_ACCESS_KEY=...\nAWS_DEFAULT_REGION=us-east-1',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                Semantics(
                  label: 'Values are encrypted at rest',
                  child: Row(
                    children: [
                      Icon(Icons.lock, size: 14, color: Colors.grey[500]),
                      const SizedBox(width: 4),
                      Text('Values are encrypted at rest (AES-256-GCM)',
                          style: TextStyle(
                              fontSize: 12, color: Colors.grey[500])),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx),
              child: const Text('Cancel')),
          FilledButton(
            onPressed: () async {
              final id = isEdit ? existing.id : idCtrl.text.trim();
              if (id.isEmpty || nameCtrl.text.trim().isEmpty) return;

              // Parse KEY=VALUE pairs
              final values = <String, String>{};
              for (final line in valuesCtrl.text.split('\n')) {
                final trimmed = line.trim();
                if (trimmed.isEmpty) continue;
                final eqIdx = trimmed.indexOf('=');
                if (eqIdx <= 0) continue;
                final key = trimmed.substring(0, eqIdx).trim();
                final value = trimmed.substring(eqIdx + 1);
                if (key.isNotEmpty && value.isNotEmpty) {
                  values[key] = value;
                }
              }

              if (values.isEmpty) {
                ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
                    content:
                        Text('At least one KEY=VALUE pair is required')));
                return;
              }

              try {
                final api = ref.read(apiClientProvider);
                if (isEdit) {
                  await api.updateCredential(
                      id: id,
                      name: nameCtrl.text.trim(),
                      description: descCtrl.text.trim(),
                      values: values);
                } else {
                  await api.createCredential(
                      id: id,
                      name: nameCtrl.text.trim(),
                      description: descCtrl.text.trim(),
                      values: values);
                }
                if (ctx.mounted) Navigator.pop(ctx);
                _refresh();
              } catch (e) {
                if (mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(content: Text('Save failed: $e')));
                }
              }
            },
            child: Text(isEdit ? 'Save' : 'Create'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Semantics(
                  header: true,
                  label: 'Credentials',
                  child: Text('Credentials',
                      style: Theme.of(context).textTheme.headlineSmall),
                ),
                const SizedBox(width: 12),
                Semantics(
                  label: '${_credentials.length} credentials',
                  child: Text('(${_credentials.length})',
                      style: TextStyle(color: Colors.grey[500])),
                ),
                const Spacer(),
                IconButton(
                    onPressed: _refresh, icon: const Icon(Icons.refresh)),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: () => _showCreateEditDialog(),
                  icon: const Icon(Icons.add),
                  label: const Text('Add Credential'),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Expanded(
              child: _loading
                  ? const Center(child: CircularProgressIndicator())
                  : _credentials.isEmpty
                      ? Center(
                          child: Semantics(
                            label: 'No credentials configured',
                            child: const Text(
                                'No credentials configured. Add credentials for tool authentication.'),
                          ),
                        )
                      : ListView.builder(
                          itemCount: _credentials.length,
                          itemBuilder: (context, i) {
                            final cred = _credentials[i];
                            return Card(
                              child: ListTile(
                                leading: const Icon(Icons.vpn_key),
                                title: Semantics(
                                  label: 'Credential ${cred.name}',
                                  child: Text(cred.name),
                                ),
                                subtitle: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(cred.id,
                                        style: TextStyle(
                                            fontSize: 12,
                                            fontFamily: 'monospace',
                                            color: Colors.grey[500])),
                                    if (cred.keys.isNotEmpty)
                                      Semantics(
                                        label: 'Keys: ${cred.keys.join(", ")}',
                                        child: Text(
                                            'Keys: ${cred.keys.join(", ")}',
                                            style: const TextStyle(fontSize: 12)),
                                      ),
                                  ],
                                ),
                                trailing: Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    IconButton(
                                      icon: const Icon(Icons.edit, size: 20),
                                      onPressed: () => _showCreateEditDialog(
                                          existing: cred),
                                    ),
                                    IconButton(
                                      icon: const Icon(Icons.delete, size: 20),
                                      onPressed: () => _delete(cred.id),
                                    ),
                                  ],
                                ),
                              ),
                            );
                          },
                        ),
            ),
          ],
        ),
      ),
    );
  }
}
