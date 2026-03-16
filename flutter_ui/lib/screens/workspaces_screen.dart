import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/workspace.dart';

class WorkspacesScreen extends ConsumerStatefulWidget {
  const WorkspacesScreen({super.key});

  @override
  ConsumerState<WorkspacesScreen> createState() => _WorkspacesScreenState();
}

class _WorkspacesScreenState extends ConsumerState<WorkspacesScreen> {
  List<Workspace> _workspaces = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final workspaces = await ref.read(apiClientProvider).listWorkspaces();
      setState(() {
        _workspaces = workspaces;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to load workspaces: $e')));
      }
    }
  }

  Future<void> _deleteWorkspace(Workspace ws) async {
    final confirm = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Workspace'),
        content: Text('Delete "${ws.name}"? Directory will be preserved on disk.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Delete')),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).deleteWorkspace(ws.id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _showCreateDialog() async {
    final nameCtrl = TextEditingController();
    final descCtrl = TextEditingController();
    final pathCtrl = TextEditingController();
    final claudeMdCtrl = TextEditingController();
    String? errorText;

    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('New Workspace'),
          content: SizedBox(
            width: 550,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
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
                    controller: pathCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Path (optional)',
                      helperText: 'Leave empty to auto-create in ~/.claw/workspaces/',
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: claudeMdCtrl,
                    decoration: const InputDecoration(
                      labelText: 'CLAUDE.md (optional)',
                      alignLabelWithHint: true,
                    ),
                    maxLines: 6,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
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
                if (nameCtrl.text.trim().isEmpty) {
                  setDialogState(() => errorText = 'Name is required');
                  return;
                }
                final data = <String, dynamic>{
                  'name': nameCtrl.text.trim(),
                  if (descCtrl.text.trim().isNotEmpty)
                    'description': descCtrl.text.trim(),
                  if (pathCtrl.text.trim().isNotEmpty)
                    'path': pathCtrl.text.trim(),
                  if (claudeMdCtrl.text.trim().isNotEmpty)
                    'claude_md': claudeMdCtrl.text.trim(),
                };
                try {
                  await ref.read(apiClientProvider).createWorkspace(data);
                  if (ctx.mounted) Navigator.pop(ctx, true);
                } catch (e) {
                  setDialogState(() => errorText = 'Failed: $e');
                }
              },
              child: const Text('Create'),
            ),
          ],
        ),
      ),
    );
    if (saved == true) _refresh();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Semantics(
                header: true,
                label: 'Workspaces',
                child: Text('Workspaces',
                    style: Theme.of(context).textTheme.headlineMedium),
              ),
              const Spacer(),
              FilledButton.icon(
                onPressed: _showCreateDialog,
                icon: const Icon(Icons.add),
                label: const Text('New Workspace'),
              ),
              const SizedBox(width: 8),
              IconButton(onPressed: _refresh, icon: const Icon(Icons.refresh)),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else if (_workspaces.isEmpty)
            const Center(
              child: Padding(
                padding: EdgeInsets.all(48),
                child: Text('No workspaces yet. Create one to organize your jobs.'),
              ),
            )
          else
            Expanded(
              child: ListView.builder(
                itemCount: _workspaces.length,
                itemBuilder: (context, i) => _buildWorkspaceTile(_workspaces[i]),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildWorkspaceTile(Workspace ws) {
    return Card(
      child: ListTile(
        leading: const Icon(Icons.folder_open, size: 32),
        title: Semantics(
          label: 'Workspace ${ws.name}',
          child: Text(ws.name,
              style: const TextStyle(fontWeight: FontWeight.bold)),
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (ws.description.isNotEmpty) Text(ws.description),
            Text(ws.path,
                style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
            Row(
              children: [
                if (ws.skillIds.isNotEmpty)
                  Text('${ws.skillIds.length} skills',
                      style: const TextStyle(fontSize: 12)),
                if (ws.claudeMd != null)
                  const Padding(
                    padding: EdgeInsets.only(left: 8),
                    child: Icon(Icons.description, size: 14),
                  ),
              ],
            ),
          ],
        ),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              icon: const Icon(Icons.open_in_new),
              tooltip: 'View Details',
              onPressed: () => context.go('/workspaces/${ws.id}'),
            ),
            IconButton(
              icon: const Icon(Icons.delete),
              tooltip: 'Delete',
              onPressed: () => _deleteWorkspace(ws),
            ),
          ],
        ),
        onTap: () => context.go('/workspaces/${ws.id}'),
      ),
    );
  }
}

/// Workspace detail/edit screen.
class WorkspaceDetailScreen extends ConsumerStatefulWidget {
  final String workspaceId;
  const WorkspaceDetailScreen({super.key, required this.workspaceId});

  @override
  ConsumerState<WorkspaceDetailScreen> createState() =>
      _WorkspaceDetailScreenState();
}

class _WorkspaceDetailScreenState
    extends ConsumerState<WorkspaceDetailScreen> {
  Workspace? _workspace;
  List<dynamic> _files = [];
  bool _loading = true;
  final _claudeMdCtrl = TextEditingController();
  final _nameCtrl = TextEditingController();
  final _descCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final api = ref.read(apiClientProvider);
      final ws = await api.getWorkspace(widget.workspaceId);
      List<dynamic> files = [];
      try {
        files = await api.listWorkspaceFiles(widget.workspaceId);
      } catch (_) {}
      setState(() {
        _workspace = ws;
        _files = files;
        _nameCtrl.text = ws.name;
        _descCtrl.text = ws.description;
        _claudeMdCtrl.text = ws.claudeMd ?? '';
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _save() async {
    try {
      await ref.read(apiClientProvider).updateWorkspace(widget.workspaceId, {
        'name': _nameCtrl.text.trim(),
        'description': _descCtrl.text.trim(),
        'claude_md': _claudeMdCtrl.text.isNotEmpty ? _claudeMdCtrl.text : null,
        'skill_ids': _workspace?.skillIds ?? [],
      });
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Saved')));
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Save failed: $e')));
      }
    }
  }

  @override
  void dispose() {
    _claudeMdCtrl.dispose();
    _nameCtrl.dispose();
    _descCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    final ws = _workspace;
    if (ws == null) {
      return const Center(child: Text('Workspace not found'));
    }

    return Padding(
      padding: const EdgeInsets.all(24),
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              children: [
                IconButton(
                  icon: const Icon(Icons.arrow_back),
                  onPressed: () => context.go('/workspaces'),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(ws.name,
                      style: Theme.of(context).textTheme.headlineMedium),
                ),
                FilledButton.icon(
                  onPressed: _save,
                  icon: const Icon(Icons.save),
                  label: const Text('Save'),
                ),
                const SizedBox(width: 8),
                IconButton(
                    onPressed: _refresh, icon: const Icon(Icons.refresh)),
              ],
            ),
            const SizedBox(height: 24),

            // Metadata
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _nameCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Name',
                      border: OutlineInputBorder(),
                    ),
                  ),
                ),
                const SizedBox(width: 16),
                Expanded(
                  child: TextField(
                    controller: _descCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Description',
                      border: OutlineInputBorder(),
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text('Path: ${ws.path}',
                style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
            const SizedBox(height: 24),

            // CLAUDE.md Editor
            Text('CLAUDE.md', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            TextField(
              controller: _claudeMdCtrl,
              maxLines: 12,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
              decoration: const InputDecoration(
                hintText:
                    'Enter CLAUDE.md content for this workspace...',
                border: OutlineInputBorder(),
                alignLabelWithHint: true,
              ),
            ),
            const SizedBox(height: 24),

            // File Browser
            Text('Files', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            if (_files.isEmpty)
              const Text('No files in workspace directory.')
            else
              Card(
                child: SizedBox(
                  height: 300,
                  child: ListView.builder(
                    itemCount: _files.length,
                    itemBuilder: (context, i) {
                      final file = _files[i];
                      final isDir = file['is_dir'] == true;
                      return ListTile(
                        dense: true,
                        leading: Icon(
                          isDir ? Icons.folder : Icons.insert_drive_file,
                          size: 18,
                        ),
                        title: Text(
                          file['path'] ?? '',
                          style: const TextStyle(
                              fontFamily: 'monospace', fontSize: 12),
                        ),
                        trailing: isDir
                            ? null
                            : Text(
                                _formatSize(file['size'] ?? 0),
                                style: const TextStyle(fontSize: 11),
                              ),
                      );
                    },
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  String _formatSize(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    return '${(bytes / 1024 / 1024).toStringAsFixed(1)} MB';
  }
}
