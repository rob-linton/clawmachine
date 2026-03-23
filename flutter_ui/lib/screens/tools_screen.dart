import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
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
  List<Map<String, dynamic>> _recommended = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      // Sync from catalog before loading
      try { await ref.read(apiClientProvider).syncCatalog(); } catch (_) {}
      final tools = await ref.read(apiClientProvider).listTools();
      // Fetch catalog recommended items
      List<Map<String, dynamic>> recommended = [];
      try {
        final catalog = await ref.read(apiClientProvider).fetchCatalog();
        final catalogItems = List<Map<String, dynamic>>.from(catalog['tools'] ?? []);
        final installedIds = tools.map((t) => t.id).toSet();
        recommended = catalogItems.where((item) => !installedIds.contains(item['id'])).toList();
      } catch (_) {
        // Catalog fetch failure is non-fatal
      }
      setState(() {
        _tools = tools;
        _recommended = recommended;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _installCatalogItem(Map<String, dynamic> item) async {
    final url = item['url'] as String? ?? '';
    final path = item['path'] as String?;
    if (url.isEmpty) return;
    try {
      await ref.read(apiClientProvider).installToolFromUrl(url: url, path: path);
      _refresh();
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Installed ${item['name'] ?? item['id']}')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Install failed: $e')));
      }
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

  Future<void> _installFromUrl() async {
    final urlCtrl = TextEditingController();
    final pathCtrl = TextEditingController();
    String? errorText;
    bool installing = false;

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: Semantics(
            header: true,
            label: 'Install Tool from URL',
            child: const Text('Install Tool from URL'),
          ),
          content: SizedBox(
            width: 500,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextField(
                    controller: urlCtrl,
                    decoration: const InputDecoration(
                      labelText: 'URL',
                      hintText: 'https://github.com/user/repo',
                      helperText: 'Git repo URL or direct .zip link (must be https://)',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: pathCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Subdirectory path (optional)',
                      hintText: 'tools/my-tool',
                      helperText: 'Path within the repo to look for TOOL.json + manifest.json',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  if (errorText != null) ...[
                    const SizedBox(height: 8),
                    Semantics(
                      label: errorText!,
                      child: Text(errorText!, style: const TextStyle(color: Colors.red)),
                    ),
                  ],
                  if (installing) ...[
                    const SizedBox(height: 12),
                    const Row(
                      children: [
                        SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2)),
                        SizedBox(width: 8),
                        Text('Cloning and installing...'),
                      ],
                    ),
                  ],
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: installing ? null : () => Navigator.pop(ctx, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: installing
                  ? null
                  : () async {
                      if (urlCtrl.text.trim().isEmpty) {
                        setDialogState(() => errorText = 'URL is required');
                        return;
                      }
                      setDialogState(() {
                        installing = true;
                        errorText = null;
                      });
                      try {
                        final path = pathCtrl.text.trim().isEmpty ? null : pathCtrl.text.trim();
                        await ref.read(apiClientProvider).installToolFromUrl(
                          url: urlCtrl.text.trim(),
                          path: path,
                        );
                        if (ctx.mounted) Navigator.pop(ctx, true);
                      } catch (e) {
                        setDialogState(() {
                          installing = false;
                          errorText = 'Install failed: $e';
                        });
                      }
                    },
              child: const Text('Install'),
            ),
          ],
        ),
      ),
    );
    if (saved == true) _refresh();
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
                  onPressed: _installFromUrl,
                  icon: const Icon(Icons.link),
                  label: const Text('Install from URL'),
                ),
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
                  : (_tools.isEmpty && _recommended.isEmpty)
                      ? Center(
                          child: Semantics(
                            label: 'No CLI tools configured yet',
                            child: const Text(
                                'No CLI tools configured yet. Click "Add Tool" to get started.'),
                          ),
                        )
                      : ListView(
                          children: [
                            if (_tools.isNotEmpty)
                              GridView.builder(
                                shrinkWrap: true,
                                physics: const NeverScrollableScrollPhysics(),
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
                                    onTap: () => context.go('/tools/${tool.id}'),
                                    onEdit: () =>
                                        _showCreateEditDialog(existing: tool),
                                    onDelete: () => _delete(tool.id),
                                    onExport: () => _exportTool(tool),
                                  );
                                },
                              ),
                            // Recommended from catalog
                            if (_recommended.isNotEmpty) ...[
                              const SizedBox(height: 24),
                              Semantics(
                                header: true,
                                label: 'Recommended',
                                child: Text('Recommended',
                                    style: Theme.of(context).textTheme.headlineSmall),
                              ),
                              const SizedBox(height: 8),
                              Semantics(
                                label: 'From curated catalog',
                                child: Text('From curated catalog',
                                    style: TextStyle(color: Colors.grey[500])),
                              ),
                              const SizedBox(height: 12),
                              ..._recommended.map((item) => Card(
                                child: ListTile(
                                  leading: const Icon(Icons.build_circle),
                                  title: Semantics(
                                    label: 'Recommended tool ${item['name'] ?? item['id']}',
                                    child: Text(item['name'] ?? item['id'] ?? ''),
                                  ),
                                  subtitle: Column(
                                    crossAxisAlignment: CrossAxisAlignment.start,
                                    children: [
                                      if ((item['description'] ?? '').toString().isNotEmpty)
                                        Semantics(
                                          label: item['description'].toString(),
                                          child: Text(item['description'].toString(),
                                              maxLines: 2, overflow: TextOverflow.ellipsis),
                                        ),
                                      if ((item['author'] ?? '').toString().isNotEmpty ||
                                          (item['version'] ?? '').toString().isNotEmpty)
                                        Semantics(
                                          label: [
                                            if ((item['version'] ?? '').toString().isNotEmpty) 'v${item['version']}',
                                            if ((item['author'] ?? '').toString().isNotEmpty) 'by ${item['author']}',
                                          ].join(' '),
                                          child: Text(
                                            [
                                              if ((item['version'] ?? '').toString().isNotEmpty) 'v${item['version']}',
                                              if ((item['author'] ?? '').toString().isNotEmpty) 'by ${item['author']}',
                                            ].join(' '),
                                            style: TextStyle(fontSize: 12, color: Colors.grey[500]),
                                          ),
                                        ),
                                    ],
                                  ),
                                  trailing: FilledButton(
                                    onPressed: () => _installCatalogItem(item),
                                    child: Semantics(
                                      label: 'Install ${item['name'] ?? item['id']}',
                                      child: const Text('Install'),
                                    ),
                                  ),
                                ),
                              )),
                            ],
                          ],
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
  final VoidCallback onTap;
  final VoidCallback onEdit;
  final VoidCallback onDelete;
  final VoidCallback onExport;

  const _ToolCard(
      {required this.tool,
      required this.onTap,
      required this.onEdit,
      required this.onDelete,
      required this.onExport});

  @override
  Widget build(BuildContext context) {
    return Opacity(
      opacity: tool.enabled ? 1.0 : 0.5,
      child: Card(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Semantics(
                    label: tool.sourceUrl != null && tool.sourceUrl!.isNotEmpty ? 'Catalog tool' : 'Tool',
                    child: Icon(
                      tool.sourceUrl != null && tool.sourceUrl!.isNotEmpty ? Icons.cloud_done : Icons.build_circle,
                      size: 20,
                      color: tool.sourceUrl != null && tool.sourceUrl!.isNotEmpty ? Colors.blue[300] : null,
                    ),
                  ),
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
              if (!tool.enabled)
                Semantics(
                  label: 'Disabled',
                  child: Chip(
                    label: const Text('Disabled', style: TextStyle(fontSize: 10)),
                    backgroundColor: Colors.red.shade900,
                    visualDensity: VisualDensity.compact,
                    padding: EdgeInsets.zero,
                  ),
                ),
              if (tool.description.isNotEmpty) ...[
                const SizedBox(height: 4),
                Text(tool.description,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 13)),
              ],
              const Spacer(),
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
    ),
    );
  }
}
