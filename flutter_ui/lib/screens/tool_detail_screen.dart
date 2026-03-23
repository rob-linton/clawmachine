import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:web/web.dart' as web;
import '../main.dart';
import '../models/tool.dart';

class ToolDetailScreen extends ConsumerStatefulWidget {
  final String toolId;
  const ToolDetailScreen({super.key, required this.toolId});

  @override
  ConsumerState<ToolDetailScreen> createState() => _ToolDetailScreenState();
}

class _ToolDetailScreenState extends ConsumerState<ToolDetailScreen> {
  Tool? _tool;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final tool = await ref.read(apiClientProvider).getTool(widget.toolId);
      setState(() {
        _tool = tool;
        _loading = false;
        _error = null;
      });
    } catch (e) {
      setState(() {
        _loading = false;
        _error = e.toString();
      });
    }
  }

  Future<void> _toggleEnabled() async {
    final tool = _tool;
    if (tool == null) return;
    final updated = Tool(
      id: tool.id,
      name: tool.name,
      description: tool.description,
      tags: tool.tags,
      installCommands: tool.installCommands,
      checkCommand: tool.checkCommand,
      envVars: tool.envVars,
      authScript: tool.authScript,
      version: tool.version,
      author: tool.author,
      license: tool.license,
      sourceUrl: tool.sourceUrl,
      enabled: !tool.enabled,
    );
    try {
      await ref.read(apiClientProvider).updateTool(tool.id, updated);
      setState(() => _tool = updated);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to update: $e')));
      }
    }
  }

  void _exportTool() {
    final tool = _tool;
    if (tool == null) return;
    final url = ref.read(apiClientProvider).toolDownloadUrl(tool.id);
    final anchor = web.document.createElement('a') as web.HTMLAnchorElement;
    anchor.href = url;
    anchor.download = '${tool.id}.zip';
    anchor.click();
  }

  Future<void> _updateFromSource() async {
    final tool = _tool;
    if (tool == null || tool.sourceUrl == null) return;
    try {
      setState(() => _loading = true);
      await ref.read(apiClientProvider).updateToolFromSource(tool.id);
      _refresh();
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Tool updated from source')));
      }
    } catch (e) {
      setState(() => _loading = false);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Update failed: $e')));
      }
    }
  }

  Future<void> _deleteTool() async {
    final confirm = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Tool'),
        content:
            const Text('Are you sure? Jobs using this tool will fail.'),
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
      await ref.read(apiClientProvider).deleteTool(widget.toolId);
      if (mounted) context.go('/tools');
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _showEditDialog() async {
    final tool = _tool;
    if (tool == null) return;

    final nameCtrl = TextEditingController(text: tool.name);
    final descCtrl = TextEditingController(text: tool.description);
    final installCtrl = TextEditingController(text: tool.installCommands);
    final checkCtrl = TextEditingController(text: tool.checkCommand);
    final authCtrl = TextEditingController(text: tool.authScript ?? '');
    final tagsCtrl = TextEditingController(text: tool.tags.join(', '));
    final versionCtrl = TextEditingController(text: tool.version);
    final authorCtrl = TextEditingController(text: tool.author);
    final envVarsCtrl = TextEditingController(
        text: tool.envVars
            .map((e) => '${e.key}: ${e.description}')
            .join('\n'));

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Semantics(
          header: true,
          label: 'Edit Tool',
          child: const Text('Edit Tool'),
        ),
        content: SizedBox(
          width: 600,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: nameCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Name', border: OutlineInputBorder()),
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
                  controller: versionCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Version', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: authorCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Author', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: installCtrl,
                  maxLines: 5,
                  decoration: const InputDecoration(
                      labelText: 'Install Commands',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: checkCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Check Command',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: envVarsCtrl,
                  maxLines: 3,
                  decoration: const InputDecoration(
                      labelText: 'Env Vars (KEY: description, one per line)',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: authCtrl,
                  maxLines: 3,
                  decoration: const InputDecoration(
                      labelText: 'Auth Script (optional)',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: tagsCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Tags (comma-separated)',
                      border: OutlineInputBorder()),
                ),
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
              final updated = Tool(
                id: tool.id,
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
                enabled: tool.enabled,
              );
              try {
                await ref
                    .read(apiClientProvider)
                    .updateTool(tool.id, updated);
                if (ctx.mounted) Navigator.pop(ctx, true);
              } catch (e) {
                if (ctx.mounted) {
                  ScaffoldMessenger.of(ctx).showSnackBar(
                      SnackBar(content: Text('Save failed: $e')));
                }
              }
            },
            child: const Text('Save'),
          ),
        ],
      ),
    );
    if (saved == true) _refresh();
  }

  String _formatDate(String? isoDate) {
    if (isoDate == null || isoDate.isEmpty) return 'N/A';
    try {
      final dt = DateTime.parse(isoDate);
      final now = DateTime.now();
      final diff = now.difference(dt);
      if (diff.inMinutes < 1) return 'just now';
      if (diff.inHours < 1) return '${diff.inMinutes}m ago';
      if (diff.inDays < 1) return '${diff.inHours}h ago';
      if (diff.inDays < 30) return '${diff.inDays}d ago';
      return '${dt.year}-${dt.month.toString().padLeft(2, '0')}-${dt.day.toString().padLeft(2, '0')}';
    } catch (_) {
      return isoDate;
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_error != null || _tool == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(_error ?? 'Tool not found'),
            const SizedBox(height: 16),
            FilledButton(
                onPressed: () => context.go('/tools'),
                child: const Text('Back to Tools')),
          ],
        ),
      );
    }

    final tool = _tool!;

    return Scaffold(
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              children: [
                IconButton(
                  icon: const Icon(Icons.arrow_back),
                  onPressed: () => context.go('/tools'),
                  tooltip: 'Back to Tools',
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Semantics(
                    header: true,
                    label: tool.name,
                    child: Text(tool.name,
                        style: Theme.of(context).textTheme.headlineMedium),
                  ),
                ),
                if (tool.version.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(left: 8),
                    child: Semantics(
                      label: 'Version ${tool.version}',
                      child: Chip(label: Text('v${tool.version}')),
                    ),
                  ),
                const SizedBox(width: 16),
                if (tool.sourceUrl != null && tool.sourceUrl!.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(right: 8),
                    child: OutlinedButton.icon(
                      onPressed: _updateFromSource,
                      icon: const Icon(Icons.refresh, size: 18),
                      label: Semantics(
                        label: 'Update from Source',
                        child: const Text('Update from Source'),
                      ),
                    ),
                  ),
                FilledButton.icon(
                  onPressed: _showEditDialog,
                  icon: const Icon(Icons.edit, size: 18),
                  label: const Text('Edit'),
                ),
                const SizedBox(width: 8),
                OutlinedButton.icon(
                  onPressed: _exportTool,
                  icon: const Icon(Icons.download, size: 18),
                  label: const Text('Export'),
                ),
                const SizedBox(width: 8),
                Semantics(
                  label: tool.enabled ? 'Enabled' : 'Disabled',
                  child: Switch(
                    value: tool.enabled,
                    onChanged: (_) => _toggleEnabled(),
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  icon: const Icon(Icons.delete_outline),
                  onPressed: _deleteTool,
                  tooltip: 'Delete',
                  color: Colors.red,
                ),
              ],
            ),
            if (tool.author.isNotEmpty || tool.license != null)
              Padding(
                padding: const EdgeInsets.only(left: 56, top: 4),
                child: Semantics(
                  label: [
                    if (tool.author.isNotEmpty) 'by ${tool.author}',
                    if (tool.license != null) tool.license!,
                  ].join(' \u2022 '),
                  child: Text(
                    [
                      if (tool.author.isNotEmpty) 'by ${tool.author}',
                      if (tool.license != null) tool.license!,
                    ].join(' \u2022 '),
                    style: TextStyle(color: Colors.grey[400], fontSize: 14),
                  ),
                ),
              ),
            const SizedBox(height: 24),
            // Body: two-column layout
            LayoutBuilder(
              builder: (context, constraints) {
                final wide = constraints.maxWidth >= 800;
                final leftColumn = _buildLeftColumn(tool);
                final rightSidebar =
                    _buildRightSidebar(tool, tool.createdAt, tool.updatedAt);

                if (wide) {
                  return Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(child: leftColumn),
                      const SizedBox(width: 24),
                      SizedBox(width: 250, child: rightSidebar),
                    ],
                  );
                } else {
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      leftColumn,
                      const SizedBox(height: 24),
                      rightSidebar,
                    ],
                  );
                }
              },
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildLeftColumn(Tool tool) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (tool.description.isNotEmpty)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Semantics(
                    header: true,
                    label: 'Description',
                    child: Text('Description',
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  const SizedBox(height: 8),
                  Semantics(
                    label: tool.description,
                    child: Text(tool.description),
                  ),
                ],
              ),
            ),
          ),
        const SizedBox(height: 16),
        // Install Commands
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Semantics(
                  header: true,
                  label: 'Install Commands',
                  child: Text('Install Commands',
                      style: Theme.of(context).textTheme.titleSmall),
                ),
                const SizedBox(height: 8),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: const Color(0xFF1E1E1E),
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: Semantics(
                    label: 'Install commands content',
                    child: SelectableText(
                      tool.installCommands.isEmpty
                          ? '(none)'
                          : tool.installCommands,
                      style: const TextStyle(
                          fontFamily: 'monospace', fontSize: 13),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 16),
        // Check Command
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              children: [
                Icon(Icons.terminal, size: 18, color: Colors.grey[400]),
                const SizedBox(width: 8),
                Semantics(
                  header: true,
                  label: 'Check Command',
                  child: Text('Check Command: ',
                      style: Theme.of(context).textTheme.titleSmall),
                ),
                Expanded(
                  child: Semantics(
                    label: 'Check command ${tool.checkCommand}',
                    child: Text(tool.checkCommand,
                        style: const TextStyle(
                            fontFamily: 'monospace', fontSize: 13)),
                  ),
                ),
              ],
            ),
          ),
        ),
        if (tool.authScript != null && tool.authScript!.isNotEmpty) ...[
          const SizedBox(height: 16),
          Card(
            child: ExpansionTile(
              title: Semantics(
                header: true,
                label: 'Auth Script',
                child: const Text('Auth Script'),
              ),
              initiallyExpanded: false,
              children: [
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: const Color(0xFF1E1E1E),
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Semantics(
                      label: 'Auth script content',
                      child: SelectableText(
                        tool.authScript!,
                        style: const TextStyle(
                            fontFamily: 'monospace', fontSize: 13),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
        if (tool.envVars.isNotEmpty) ...[
          const SizedBox(height: 16),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Semantics(
                    header: true,
                    label: 'Environment Variables',
                    child: Text('Environment Variables',
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  const SizedBox(height: 8),
                  SizedBox(
                    width: double.infinity,
                    child: DataTable(
                      columnSpacing: 24,
                      columns: const [
                        DataColumn(label: Text('Key')),
                        DataColumn(label: Text('Description')),
                        DataColumn(label: Text('Required')),
                      ],
                      rows: tool.envVars
                          .map((ev) => DataRow(cells: [
                                DataCell(Semantics(
                                  label: 'Env var ${ev.key}',
                                  child: InkWell(
                                    onTap: () {
                                      web.window.navigator.clipboard.writeText(ev.key);
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        SnackBar(content: Text('Copied ${ev.key}'), duration: const Duration(seconds: 1)),
                                      );
                                    },
                                    child: Row(
                                      mainAxisSize: MainAxisSize.min,
                                      children: [
                                        Text(ev.key,
                                            style: const TextStyle(
                                                fontFamily: 'monospace',
                                                fontSize: 13)),
                                        const SizedBox(width: 4),
                                        Icon(Icons.copy, size: 14, color: Colors.grey[500]),
                                      ],
                                    ),
                                  ),
                                )),
                                DataCell(Semantics(
                                  label: ev.description,
                                  child: Text(ev.description),
                                )),
                                DataCell(Semantics(
                                  label: ev.required
                                      ? 'Required'
                                      : 'Optional',
                                  child: Icon(
                                    ev.required
                                        ? Icons.check_box
                                        : Icons.check_box_outline_blank,
                                    size: 18,
                                    color: ev.required
                                        ? Colors.green
                                        : Colors.grey,
                                  ),
                                )),
                              ]))
                          .toList(),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildRightSidebar(
      Tool tool, String? createdAt, String? updatedAt) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _sidebarRow('ID', tool.id, mono: true),
            _sidebarRow(
                'Version', tool.version.isEmpty ? 'N/A' : tool.version),
            _sidebarRow(
                'Author', tool.author.isEmpty ? 'N/A' : tool.author),
            _sidebarRow('License', tool.license ?? 'N/A'),
            if (tool.sourceUrl != null && tool.sourceUrl!.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Source URL',
                        style: TextStyle(
                            fontSize: 11, color: Colors.grey[500])),
                    Semantics(
                      label: 'Source URL ${tool.sourceUrl}',
                      child: InkWell(
                        onTap: () {},
                        child: Text(tool.sourceUrl!,
                            style: const TextStyle(
                                fontSize: 13,
                                color: Colors.blue,
                                decoration: TextDecoration.underline)),
                      ),
                    ),
                  ],
                ),
              ),
            _sidebarRow('Created', _formatDate(createdAt)),
            _sidebarRow('Updated', _formatDate(updatedAt)),
            const Divider(),
            if (tool.tags.isNotEmpty) ...[
              Semantics(
                header: true,
                label: 'Tags',
                child: Text('Tags',
                    style:
                        TextStyle(fontSize: 11, color: Colors.grey[500])),
              ),
              const SizedBox(height: 4),
              Wrap(
                spacing: 4,
                runSpacing: 4,
                children: tool.tags
                    .map((t) => Semantics(
                          label: 'Tag $t',
                          child: Chip(
                            label: Text(t,
                                style: const TextStyle(fontSize: 10)),
                            visualDensity: VisualDensity.compact,
                            padding: EdgeInsets.zero,
                          ),
                        ))
                    .toList(),
              ),
              const SizedBox(height: 8),
            ],
            Semantics(
              label:
                  tool.enabled ? 'Status: Enabled' : 'Status: Disabled',
              child: Chip(
                label: Text(tool.enabled ? 'Enabled' : 'Disabled'),
                backgroundColor: tool.enabled
                    ? Colors.green.shade900
                    : Colors.red.shade900,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _sidebarRow(String label, String value, {bool mono = false}) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label,
              style: TextStyle(fontSize: 11, color: Colors.grey[500])),
          Semantics(
            label: '$label $value',
            child: Text(value,
                style: TextStyle(
                    fontSize: 13,
                    fontFamily: mono ? 'monospace' : null)),
          ),
        ],
      ),
    );
  }
}
