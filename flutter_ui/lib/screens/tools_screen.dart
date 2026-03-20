import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../main.dart';
import '../models/tool.dart';
import '../services/file_upload.dart';

class ToolsScreen extends ConsumerStatefulWidget {
  const ToolsScreen({super.key});

  @override
  ConsumerState<ToolsScreen> createState() => _ToolsScreenState();
}

class _ToolsScreenState extends ConsumerState<ToolsScreen> {
  List<Tool> _tools = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final tools = await ref.read(apiClientProvider).listTools();
      setState(() {
        _tools = tools;
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
        title: const Text('Delete Tool'),
        content: const Text('Are you sure? Jobs using this tool will fail.'),
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
      await ref.read(apiClientProvider).deleteTool(id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  void _exportTool(Tool tool) {
    final url = ref.read(apiClientProvider).toolDownloadUrl(tool.id);
    final anchor = web.document.createElement('a') as web.HTMLAnchorElement;
    anchor.href = url;
    anchor.download = '${tool.id}.zip';
    anchor.click();
  }

  Future<void> _importToolZip() async {
    final PickedFile file;
    try {
      final picked = await pickFile(accept: '.zip');
      if (picked == null) return;
      file = picked;
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('File picker error: $e')));
      }
      return;
    }

    if (!file.name.endsWith('.zip')) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Please select a .zip file')));
      }
      return;
    }

    final bytes = file.bytes;

    final idCtrl = TextEditingController();
    final nameCtrl = TextEditingController();
    final descCtrl = TextEditingController();
    final tagsCtrl = TextEditingController();
    String? errorText;

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: Text('Import ${file.name}'),
          content: SizedBox(
            width: 500,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text('${(bytes.length / 1024).toStringAsFixed(1)} KB',
                      style: const TextStyle(color: Colors.grey)),
                  const SizedBox(height: 12),
                  TextField(
                    controller: idCtrl,
                    decoration: const InputDecoration(labelText: 'Tool ID'),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: nameCtrl,
                    decoration: const InputDecoration(labelText: 'Name'),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: descCtrl,
                    decoration: const InputDecoration(labelText: 'Description'),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: tagsCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Tags (comma-separated)',
                    ),
                  ),
                  if (errorText != null) ...[
                    const SizedBox(height: 8),
                    Text(errorText!, style: const TextStyle(color: Colors.red)),
                  ],
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx, false),
                child: const Text('Cancel')),
            FilledButton(
              onPressed: () async {
                if (idCtrl.text.trim().isEmpty || nameCtrl.text.trim().isEmpty) {
                  setDialogState(() => errorText = 'ID and Name are required');
                  return;
                }
                try {
                  final tags = tagsCtrl.text
                      .split(',')
                      .map((t) => t.trim())
                      .where((t) => t.isNotEmpty)
                      .toList();
                  await ref.read(apiClientProvider).uploadToolZip(
                    Uint8List.fromList(bytes),
                    id: idCtrl.text.trim(),
                    name: nameCtrl.text.trim(),
                    description: descCtrl.text.trim(),
                    tags: tags,
                  );
                  if (ctx.mounted) Navigator.pop(ctx, true);
                } catch (e) {
                  setDialogState(() => errorText = 'Failed: $e');
                }
              },
              child: const Text('Import'),
            ),
          ],
        ),
      ),
    );
    if (saved == true) _refresh();
  }

  Future<void> _showCreateEditDialog({Tool? existing}) async {
    final idCtrl = TextEditingController(text: existing?.id ?? '');
    final nameCtrl = TextEditingController(text: existing?.name ?? '');
    final descCtrl = TextEditingController(text: existing?.description ?? '');
    final installCtrl =
        TextEditingController(text: existing?.installCommands ?? '');
    final checkCtrl = TextEditingController(text: existing?.checkCommand ?? '');
    final authCtrl = TextEditingController(text: existing?.authScript ?? '');
    final tagsCtrl =
        TextEditingController(text: existing?.tags.join(', ') ?? '');
    final envVarsCtrl = TextEditingController(
        text: existing?.envVars.map((e) => '${e.key}: ${e.description}').join('\n') ?? '');
    final versionCtrl = TextEditingController(text: existing?.version ?? '');
    final authorCtrl = TextEditingController(text: existing?.author ?? '');

    final isEdit = existing != null;

    await showDialog(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Semantics(
          header: true,
          label: isEdit ? 'Edit Tool' : 'Create Tool',
          child: Text(isEdit ? 'Edit Tool' : 'Create Tool'),
        ),
        content: SizedBox(
          width: 600,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (!isEdit)
                  TextField(
                    controller: idCtrl,
                    decoration: const InputDecoration(
                        labelText: 'ID (e.g., az-cli)', border: OutlineInputBorder()),
                  ),
                if (!isEdit) const SizedBox(height: 12),
                TextField(
                  controller: nameCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Name (e.g., Azure CLI)',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: descCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Description', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: versionCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Version (e.g., 1.0.0)',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: authorCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Author',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: installCtrl,
                  maxLines: 5,
                  decoration: const InputDecoration(
                      labelText: 'Install Commands (Docker/Debian)',
                      hintText:
                          'apt-get update\napt-get install -y azure-cli\nrm -rf /var/lib/apt/lists/*',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: checkCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Check Command (exit 0 = installed)',
                      hintText: 'az --version',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: envVarsCtrl,
                  maxLines: 3,
                  decoration: const InputDecoration(
                      labelText: 'Env Vars (one per line: KEY: description)',
                      hintText:
                          'AZURE_CLIENT_ID: Service principal app ID\nAZURE_CLIENT_SECRET: Service principal secret',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: authCtrl,
                  maxLines: 3,
                  decoration: const InputDecoration(
                      labelText: 'Auth Script (optional, runs before job)',
                      hintText:
                          'az login --service-principal -u \$AZURE_CLIENT_ID -p \$AZURE_CLIENT_SECRET -t \$AZURE_TENANT_ID',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: tagsCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Tags (comma-separated)',
                      hintText: 'cloud, azure',
                      border: OutlineInputBorder()),
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

              final tags = tagsCtrl.text
                  .split(',')
                  .map((t) => t.trim())
                  .where((t) => t.isNotEmpty)
                  .toList();

              final envVars = envVarsCtrl.text
                  .split('\n')
                  .where((l) => l.contains(':'))
                  .map((l) {
                final parts = l.split(':');
                return ToolEnvVar(
                    key: parts[0].trim(),
                    description: parts.sublist(1).join(':').trim());
              }).toList();

              final tool = Tool(
                id: id,
                name: nameCtrl.text.trim(),
                description: descCtrl.text.trim(),
                installCommands: installCtrl.text.trim(),
                checkCommand: checkCtrl.text.trim(),
                envVars: envVars,
                authScript: authCtrl.text.trim().isEmpty
                    ? null
                    : authCtrl.text.trim(),
                tags: tags,
                version: versionCtrl.text.trim(),
                author: authorCtrl.text.trim(),
              );

              try {
                final api = ref.read(apiClientProvider);
                if (isEdit) {
                  await api.updateTool(id, tool);
                } else {
                  await api.createTool(tool);
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
                  label: 'CLI Tools',
                  child: Text('CLI Tools',
                      style: Theme.of(context).textTheme.headlineSmall),
                ),
                const SizedBox(width: 12),
                Semantics(
                  label: '${_tools.length} tools',
                  child: Text('(${_tools.length})',
                      style: TextStyle(color: Colors.grey[500])),
                ),
                const Spacer(),
                IconButton(
                    onPressed: _refresh, icon: const Icon(Icons.refresh)),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: _importToolZip,
                  icon: const Icon(Icons.archive),
                  label: const Text('Import Tool (ZIP)'),
                ),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: () => _showCreateEditDialog(),
                  icon: const Icon(Icons.add),
                  label: const Text('Add Tool'),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Expanded(
              child: _loading
                  ? const Center(child: CircularProgressIndicator())
                  : _tools.isEmpty
                      ? Center(
                          child: Semantics(
                            label: 'No CLI tools configured yet',
                            child: const Text(
                                'No CLI tools configured yet. Click "Add Tool" to get started.'),
                          ),
                        )
                      : GridView.builder(
                          gridDelegate:
                              const SliverGridDelegateWithMaxCrossAxisExtent(
                            maxCrossAxisExtent: 400,
                            mainAxisExtent: 220,
                            crossAxisSpacing: 16,
                            mainAxisSpacing: 16,
                          ),
                          itemCount: _tools.length,
                          itemBuilder: (context, i) {
                            final tool = _tools[i];
                            return _ToolCard(
                              tool: tool,
                              onEdit: () =>
                                  _showCreateEditDialog(existing: tool),
                              onDelete: () => _delete(tool.id),
                              onExport: () => _exportTool(tool),
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

class _ToolCard extends StatelessWidget {
  final Tool tool;
  final VoidCallback onEdit;
  final VoidCallback onDelete;
  final VoidCallback onExport;

  const _ToolCard(
      {required this.tool,
      required this.onEdit,
      required this.onDelete,
      required this.onExport});

  @override
  Widget build(BuildContext context) {
    return Card(
      child: InkWell(
        onTap: onEdit,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.build_circle, size: 20),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Semantics(
                      label: 'Tool ${tool.name}',
                      child: Text(tool.name,
                          style: const TextStyle(
                              fontWeight: FontWeight.bold, fontSize: 16),
                          overflow: TextOverflow.ellipsis),
                    ),
                  ),
                  PopupMenuButton<String>(
                    itemBuilder: (_) => [
                      const PopupMenuItem(value: 'edit', child: Text('Edit')),
                      const PopupMenuItem(
                          value: 'export', child: Text('Export')),
                      const PopupMenuItem(
                          value: 'delete', child: Text('Delete')),
                    ],
                    onSelected: (v) {
                      if (v == 'edit') onEdit();
                      if (v == 'export') onExport();
                      if (v == 'delete') onDelete();
                    },
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Semantics(
                label: 'Tool ID ${tool.id}',
                child: Text(tool.id,
                    style: TextStyle(
                        fontSize: 12,
                        fontFamily: 'monospace',
                        color: Colors.grey[500])),
              ),
              if (tool.version.isNotEmpty || tool.author.isNotEmpty) ...[
                const SizedBox(height: 2),
                Semantics(
                  label: [
                    if (tool.version.isNotEmpty) 'v${tool.version}',
                    if (tool.author.isNotEmpty) 'by ${tool.author}',
                  ].join(' '),
                  child: Text(
                    [
                      if (tool.version.isNotEmpty) 'v${tool.version}',
                      if (tool.author.isNotEmpty) 'by ${tool.author}',
                    ].join(' '),
                    style: TextStyle(fontSize: 11, color: Colors.grey[500]),
                  ),
                ),
              ],
              if (tool.description.isNotEmpty) ...[
                const SizedBox(height: 4),
                Text(tool.description,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 13)),
              ],
              const Spacer(),
              Row(
                children: [
                  Icon(Icons.terminal, size: 14, color: Colors.grey[500]),
                  const SizedBox(width: 4),
                  Expanded(
                    child: Semantics(
                      label: 'Check: ${tool.checkCommand}',
                      child: Text(tool.checkCommand,
                          style: TextStyle(
                              fontSize: 11,
                              fontFamily: 'monospace',
                              color: Colors.grey[500]),
                          overflow: TextOverflow.ellipsis),
                    ),
                  ),
                ],
              ),
              if (tool.tags.isNotEmpty) ...[
                const SizedBox(height: 4),
                Wrap(
                  spacing: 4,
                  children: tool.tags
                      .map((t) => Chip(
                            label: Text(t, style: const TextStyle(fontSize: 10)),
                            visualDensity: VisualDensity.compact,
                            padding: EdgeInsets.zero,
                          ))
                      .toList(),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
