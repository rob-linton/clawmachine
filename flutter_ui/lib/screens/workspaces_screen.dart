import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/file_tree_node.dart';
import '../models/skill.dart';
import '../models/workspace.dart';
import '../services/file_upload.dart';
import '../widgets/file_tree.dart';
import '../widgets/skill_selector.dart';

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
      barrierDismissible: false,
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
    final remoteUrlCtrl = TextEditingController();
    final baseImageCtrl = TextEditingController();
    final claudeMdCtrl = TextEditingController();
    final selectedSkills = <String>{};
    String persistence = 'persistent';
    bool showLegacyPath = false;
    List<Skill> skills = [];
    try { skills = await ref.read(apiClientProvider).listSkills(); } catch (_) {}
    String? errorText;

    final saved = await showDialog<bool>(
      barrierDismissible: false,
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
                  const SizedBox(height: 16),
                  // Persistence mode selector
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Text('Persistence Mode', style: TextStyle(fontSize: 12, color: Colors.grey[400])),
                  ),
                  const SizedBox(height: 4),
                  Semantics(
                    label: 'Persistence mode selector',
                    child: SegmentedButton<String>(
                      segments: const [
                        ButtonSegment(value: 'ephemeral', label: Text('Ephemeral'), icon: Icon(Icons.refresh, size: 16)),
                        ButtonSegment(value: 'persistent', label: Text('Persistent'), icon: Icon(Icons.save, size: 16)),
                        ButtonSegment(value: 'snapshot', label: Text('Snapshot'), icon: Icon(Icons.photo_camera, size: 16)),
                      ],
                      selected: {persistence},
                      onSelectionChanged: (sel) => setDialogState(() => persistence = sel.first),
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    persistence == 'ephemeral'
                        ? 'Fresh clone each job. Claude\'s changes are discarded.'
                        : persistence == 'persistent'
                            ? 'Changes accumulate across jobs. Full git history.'
                            : 'Fresh clone from a base tag. Optionally promote results.',
                    style: TextStyle(fontSize: 12, color: Colors.grey[500]),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: remoteUrlCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Remote URL (optional)',
                      helperText: 'Git repo to clone as workspace base (e.g. https://github.com/org/repo.git)',
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: baseImageCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Base Image (optional)',
                      helperText: 'Docker image override. Leave blank for default sandbox.',
                    ),
                  ),
                  const SizedBox(height: 12),
                  SkillSelector(
                    availableSkills: skills,
                    selectedIds: selectedSkills,
                    label: 'Default Skills',
                    onChanged: (ids) => setDialogState(() {
                      selectedSkills.clear();
                      selectedSkills.addAll(ids);
                    }),
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
                  const SizedBox(height: 8),
                  // Legacy path toggle
                  InkWell(
                    onTap: () => setDialogState(() => showLegacyPath = !showLegacyPath),
                    child: Row(
                      children: [
                        Icon(showLegacyPath ? Icons.expand_less : Icons.expand_more, size: 16),
                        const SizedBox(width: 4),
                        Text('Legacy mode (explicit path)', style: TextStyle(fontSize: 12, color: Colors.grey[500])),
                      ],
                    ),
                  ),
                  if (showLegacyPath) ...[
                    const SizedBox(height: 8),
                    TextField(
                      controller: pathCtrl,
                      decoration: const InputDecoration(
                        labelText: 'Path',
                        helperText: 'Sets a fixed disk path instead of using git repos.',
                      ),
                    ),
                  ],
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
                  'persistence': persistence,
                  if (remoteUrlCtrl.text.trim().isNotEmpty)
                    'remote_url': remoteUrlCtrl.text.trim(),
                  if (baseImageCtrl.text.trim().isNotEmpty)
                    'base_image': baseImageCtrl.text.trim(),
                  if (claudeMdCtrl.text.trim().isNotEmpty)
                    'claude_md': claudeMdCtrl.text.trim(),
                  if (selectedSkills.isNotEmpty)
                    'skill_ids': selectedSkills.toList(),
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

  Color _persistenceColor(String mode) {
    switch (mode) {
      case 'ephemeral': return Colors.blue;
      case 'snapshot': return Colors.orange;
      default: return Colors.green;
    }
  }

  IconData _persistenceIcon(String mode) {
    switch (mode) {
      case 'ephemeral': return Icons.refresh;
      case 'snapshot': return Icons.photo_camera;
      default: return Icons.save;
    }
  }

  Widget _buildWorkspaceTile(Workspace ws) {
    return Card(
      child: ListTile(
        leading: const Icon(Icons.folder_open, size: 32),
        title: Semantics(
          label: 'Workspace ${ws.name}',
          child: Row(
            children: [
              Text(ws.name, style: const TextStyle(fontWeight: FontWeight.bold)),
              const SizedBox(width: 8),
              Semantics(
                label: 'Mode ${ws.persistence}',
                child: Chip(
                  materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  visualDensity: VisualDensity.compact,
                  avatar: Icon(_persistenceIcon(ws.persistence), size: 14, color: _persistenceColor(ws.persistence)),
                  label: Text(ws.persistence, style: const TextStyle(fontSize: 11)),
                  padding: EdgeInsets.zero,
                ),
              ),
              if (ws.remoteUrl != null && ws.remoteUrl!.isNotEmpty) ...[
                const SizedBox(width: 4),
                Tooltip(message: ws.remoteUrl!, child: const Icon(Icons.cloud, size: 16, color: Colors.grey)),
              ],
            ],
          ),
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (ws.description.isNotEmpty) Text(ws.description),
            if (ws.parentWorkspaceId != null)
              Semantics(
                label: 'Forked from ${ws.parentName ?? "deleted workspace"}',
                child: Text('Forked from ${ws.parentName ?? "(deleted)"}',
                    style: TextStyle(fontSize: 12, color: Colors.grey[600])),
              ),
            if (ws.isLegacy)
              Text(ws.path!, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
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
            if (!ws.isLegacy)
              IconButton(
                icon: const Icon(Icons.fork_right),
                tooltip: 'Fork workspace',
                onPressed: () => _showForkDialog(ws),
              ),
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

  Future<void> _showForkDialog(Workspace parent) async {
    final nameCtrl = TextEditingController(text: '${parent.name} (fork)');
    final descCtrl = TextEditingController(text: parent.description);
    String persistence = parent.persistence;

    final result = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('Fork Workspace'),
          content: SizedBox(
            width: 500,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Semantics(
                  label: 'Forking from ${parent.name}',
                  child: Chip(
                    avatar: const Icon(Icons.fork_right, size: 16),
                    label: Text('From: ${parent.name}'),
                  ),
                ),
                const SizedBox(height: 16),
                TextField(
                  controller: nameCtrl,
                  decoration: const InputDecoration(labelText: 'Name', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: descCtrl,
                  decoration: const InputDecoration(labelText: 'Description', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                SegmentedButton<String>(
                  segments: const [
                    ButtonSegment(value: 'ephemeral', icon: Icon(Icons.refresh, size: 16), label: Text('Ephemeral')),
                    ButtonSegment(value: 'persistent', icon: Icon(Icons.save, size: 16), label: Text('Persistent')),
                    ButtonSegment(value: 'snapshot', icon: Icon(Icons.photo_camera, size: 16), label: Text('Snapshot')),
                  ],
                  selected: {persistence},
                  onSelectionChanged: (v) => setDialogState(() => persistence = v.first),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
            FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Fork')),
          ],
        ),
      ),
    );

    if (result != true) return;

    try {
      await ref.read(apiClientProvider).forkWorkspace(parent.id, {
        'name': nameCtrl.text.trim(),
        'description': descCtrl.text.trim(),
        'persistence': persistence,
      });
      _refresh();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Forked "${parent.name}" successfully')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Fork failed: $e')),
        );
      }
    }
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
  List<FileTreeNode> _treeRoots = [];
  List<dynamic> _commits = [];
  List<dynamic> _events = [];
  int _eventsTotal = 0;
  bool _loading = true;
  String? _selectedFolderPath;
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
      List<dynamic> commits = [];
      try {
        files = await api.listWorkspaceFiles(widget.workspaceId);
      } catch (_) {}
      try {
        commits = await api.getWorkspaceHistory(widget.workspaceId);
      } catch (_) {}
      List<dynamic> events = [];
      int eventsTotal = 0;
      try {
        final evData = await api.listWorkspaceEvents(widget.workspaceId);
        events = evData['events'] as List? ?? [];
        eventsTotal = evData['total'] as int? ?? 0;
      } catch (_) {}
      final expanded = FileTreeNode.collectExpanded(_treeRoots);
      final newRoots = FileTreeNode.buildTree(files);
      FileTreeNode.restoreExpanded(newRoots, expanded);
      if (_selectedFolderPath != null && !_folderExistsInTree(newRoots, _selectedFolderPath!)) {
        _selectedFolderPath = null;
      }
      setState(() {
        _workspace = ws;
        _treeRoots = newRoots;
        _commits = commits;
        _events = events;
        _eventsTotal = eventsTotal;
        _nameCtrl.text = ws.name;
        _descCtrl.text = ws.description;
        _claudeMdCtrl.text = ws.claudeMd ?? '';
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  bool _folderExistsInTree(List<FileTreeNode> roots, String path) {
    for (final node in roots) {
      if (node.isDir && node.fullPath == path) return true;
      if (node.isDir && _folderExistsInTree(node.children, path)) return true;
    }
    return false;
  }

  void _onFolderSelected(String? folderPath) {
    setState(() => _selectedFolderPath = folderPath);
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
                  child: Row(
                    children: [
                      Text(ws.name,
                          style: Theme.of(context).textTheme.headlineMedium),
                      const SizedBox(width: 12),
                      Semantics(
                        label: 'Persistence mode ${ws.persistence}',
                        child: Chip(
                          avatar: Icon(
                            ws.persistence == 'ephemeral' ? Icons.refresh
                                : ws.persistence == 'snapshot' ? Icons.photo_camera
                                : Icons.save,
                            size: 16,
                          ),
                          label: Text(ws.persistence),
                        ),
                      ),
                    ],
                  ),
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
            Row(
              children: [
                if (ws.isLegacy) ...[
                  Expanded(
                    child: SelectableText('Path: ${ws.path}',
                        style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
                  ),
                  IconButton(
                    icon: const Icon(Icons.copy, size: 16),
                    tooltip: 'Copy path',
                    onPressed: () {
                      Clipboard.setData(ClipboardData(text: ws.path ?? ''));
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Path copied'), duration: Duration(seconds: 1)),
                      );
                    },
                  ),
                ] else ...[
                  if (ws.remoteUrl != null && ws.remoteUrl!.isNotEmpty) ...[
                    const Icon(Icons.cloud, size: 16, color: Colors.grey),
                    const SizedBox(width: 4),
                    Expanded(
                      child: SelectableText(ws.remoteUrl!,
                          style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
                    ),
                    const SizedBox(width: 8),
                    OutlinedButton.icon(
                      onPressed: _syncWorkspace,
                      icon: const Icon(Icons.sync, size: 16),
                      label: const Text('Sync'),
                    ),
                  ] else
                    const Expanded(
                      child: Text('Git-backed workspace (local bare repo)',
                          style: TextStyle(fontFamily: 'monospace', fontSize: 12)),
                    ),
                  if (ws.baseImage != null && ws.baseImage!.isNotEmpty) ...[
                    const SizedBox(width: 16),
                    Chip(
                      avatar: const Icon(Icons.inventory_2, size: 14),
                      label: Text(ws.baseImage!, style: const TextStyle(fontSize: 11)),
                      visualDensity: VisualDensity.compact,
                    ),
                  ],
                ],
              ],
            ),
            const SizedBox(height: 8),

            // Lineage + Docker config metadata
            Wrap(
              spacing: 8,
              runSpacing: 4,
              children: [
                if (ws.parentWorkspaceId != null) ...[
                  Semantics(
                    label: 'Forked from ${ws.parentName ?? "(deleted)"}',
                    child: ActionChip(
                      avatar: const Icon(Icons.fork_right, size: 16),
                      label: Text('Forked from ${ws.parentName ?? "(deleted)"}',
                          style: const TextStyle(fontSize: 11)),
                      onPressed: ws.parentName != null
                          ? () => context.go('/workspaces/${ws.parentWorkspaceId}')
                          : null,
                    ),
                  ),
                  if (ws.parentRef != null)
                    Semantics(
                      label: 'Fork point ${ws.parentRef}',
                      child: Chip(
                        avatar: const Icon(Icons.commit, size: 14),
                        label: Text(ws.parentRef!.length > 7 ? ws.parentRef!.substring(0, 7) : ws.parentRef!,
                            style: const TextStyle(fontSize: 11, fontFamily: 'monospace')),
                        visualDensity: VisualDensity.compact,
                      ),
                    ),
                ],
                if (ws.childCount > 0)
                  Semantics(
                    label: '${ws.childCount} child workspaces',
                    child: Chip(
                      avatar: const Icon(Icons.account_tree, size: 14),
                      label: Text('${ws.childCount} children', style: const TextStyle(fontSize: 11)),
                      visualDensity: VisualDensity.compact,
                    ),
                  ),
                if (ws.skillNames.isNotEmpty)
                  Semantics(
                    label: 'Skills: ${ws.skillNames.join(", ")}',
                    child: Chip(
                      avatar: const Icon(Icons.psychology, size: 14),
                      label: Text(ws.skillNames.join(', '), style: const TextStyle(fontSize: 11)),
                      visualDensity: VisualDensity.compact,
                    ),
                  ),
                Semantics(
                  label: 'Network mode ${ws.networkMode ?? "bridge"}',
                  child: Chip(
                    avatar: Icon(
                      ws.networkMode == 'none' ? Icons.wifi_off : Icons.wifi,
                      size: 14,
                      color: ws.networkMode == 'none' ? Colors.red : Colors.green,
                    ),
                    label: Text('Network: ${ws.networkMode ?? "bridge"}', style: const TextStyle(fontSize: 11)),
                    visualDensity: VisualDensity.compact,
                  ),
                ),
                if (ws.memoryLimit != null)
                  Semantics(
                    label: 'Memory limit ${ws.memoryLimit}',
                    child: Chip(
                      avatar: const Icon(Icons.memory, size: 14),
                      label: Text('Memory: ${ws.memoryLimit}', style: const TextStyle(fontSize: 11)),
                      visualDensity: VisualDensity.compact,
                    ),
                  ),
                if (ws.cpuLimit != null)
                  Semantics(
                    label: 'CPU limit ${ws.cpuLimit}',
                    child: Chip(
                      avatar: const Icon(Icons.speed, size: 14),
                      label: Text('CPU: ${ws.cpuLimit}', style: const TextStyle(fontSize: 11)),
                      visualDensity: VisualDensity.compact,
                    ),
                  ),
              ],
            ),
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
            Row(
              children: [
                Text('Files', style: Theme.of(context).textTheme.titleMedium),
                const Spacer(),
                OutlinedButton.icon(
                  onPressed: () => _showNewFileDialog(initialPath: _selectedFolderPath),
                  icon: const Icon(Icons.add_circle_outline, size: 16),
                  label: const Text('New File'),
                ),
                const SizedBox(width: 8),
                OutlinedButton.icon(
                  onPressed: () => _showNewFolderDialog(parentPath: _selectedFolderPath),
                  icon: const Icon(Icons.folder, size: 16),
                  label: const Text('New Folder'),
                ),
                const SizedBox(width: 8),
                OutlinedButton.icon(
                  onPressed: () => _uploadFileToFolder(_selectedFolderPath),
                  icon: const Icon(Icons.upload_file, size: 16),
                  label: const Text('Upload File'),
                ),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: () => _uploadZip(folderPath: _selectedFolderPath),
                  icon: const Icon(Icons.archive, size: 16),
                  label: const Text('Upload ZIP'),
                ),
              ],
            ),
            if (_selectedFolderPath != null)
              Padding(
                padding: const EdgeInsets.only(top: 4, bottom: 4),
                child: Row(
                  children: [
                    Icon(Icons.subdirectory_arrow_right, size: 14, color: Colors.grey[600]),
                    const SizedBox(width: 4),
                    Semantics(
                      label: 'Selected folder $_selectedFolderPath',
                      child: Text(
                        'Target: $_selectedFolderPath/',
                        style: TextStyle(fontSize: 12, color: Colors.grey[600], fontFamily: 'monospace'),
                      ),
                    ),
                    const SizedBox(width: 8),
                    InkWell(
                      onTap: () => _onFolderSelected(null),
                      child: Semantics(
                        label: 'Clear folder selection',
                        child: Icon(Icons.close, size: 14, color: Colors.grey[600]),
                      ),
                    ),
                  ],
                ),
              )
            else
              const SizedBox(height: 8),
            Card(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 500),
                child: SingleChildScrollView(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: FileTree(
                      roots: _treeRoots,
                      selectedFolderPath: _selectedFolderPath,
                      onFolderSelected: _onFolderSelected,
                      onFileTap: _showFileEditor,
                      onDelete: _confirmDelete,
                      onUploadToFolder: (folder) => _uploadFileToFolder(folder),
                      onNewFileInFolder: (folder) => _showNewFileDialog(initialPath: folder),
                    ),
                  ),
                ),
              ),
            ),

            // Timeline + Git History
            const SizedBox(height: 24),
            Text('History', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            DefaultTabController(
              length: ws.persistence != 'ephemeral' ? 2 : 1,
              child: Column(
                children: [
                  TabBar(
                    isScrollable: true,
                    tabs: [
                      Tab(text: _eventsTotal > 0 ? 'Timeline ($_eventsTotal)' : 'Timeline'),
                      if (ws.persistence != 'ephemeral')
                        const Tab(text: 'Git Commits'),
                    ],
                  ),
                  SizedBox(
                    height: 300,
                    child: TabBarView(
                      children: [
                        // Tab 1: Timeline (application events)
                        _events.isEmpty
                            ? const Center(child: Text('No events yet.'))
                            : ListView.builder(
                                itemCount: _events.length,
                                itemBuilder: (context, i) {
                                  final ev = _events[i] as Map<String, dynamic>;
                                  final eventType = ev['event_type'] ?? '';
                                  final desc = ev['description'] ?? '';
                                  final ts = ev['timestamp'] ?? '';
                                  final relId = ev['related_id'];
                                  final dateStr = ts.length >= 16 ? ts.substring(0, 16).replaceFirst('T', ' ') : ts;
                                  return ListTile(
                                    dense: true,
                                    leading: Icon(_eventIcon(eventType), size: 18, color: _eventColor(eventType)),
                                    title: Semantics(
                                      label: desc,
                                      child: Text(desc, style: const TextStyle(fontSize: 13), maxLines: 2, overflow: TextOverflow.ellipsis),
                                    ),
                                    subtitle: Text(dateStr, style: const TextStyle(fontSize: 11)),
                                    trailing: relId != null && (eventType == 'job_started' || eventType == 'job_completed' || eventType == 'job_failed')
                                        ? IconButton(
                                            icon: const Icon(Icons.open_in_new, size: 16),
                                            tooltip: 'View job',
                                            onPressed: () => context.go('/jobs/$relId'),
                                          )
                                        : relId != null && (eventType == 'forked' || eventType == 'child_forked')
                                            ? IconButton(
                                                icon: const Icon(Icons.open_in_new, size: 16),
                                                tooltip: 'View workspace',
                                                onPressed: () => context.go('/workspaces/$relId'),
                                              )
                                            : null,
                                  );
                                },
                              ),
                        // Tab 2: Git Commits
                        if (ws.persistence != 'ephemeral')
                          _commits.isEmpty
                              ? const Center(child: Text('No git history yet.'))
                              : ListView.builder(
                                  itemCount: _commits.length,
                                  itemBuilder: (context, i) {
                                    final commit = _commits[i];
                                    final hash = (commit['hash'] ?? '').toString();
                                    final message = commit['message'] ?? '';
                                    final date = commit['date'] ?? '';
                                    final shortHash = hash.length >= 7 ? hash.substring(0, 7) : hash;
                                    return ListTile(
                                      dense: true,
                                      leading: const Icon(Icons.commit, size: 18),
                                      title: Text(message, style: const TextStyle(fontSize: 13)),
                                      subtitle: Text('$shortHash — $date',
                                          style: const TextStyle(fontFamily: 'monospace', fontSize: 11)),
                                      trailing: message.startsWith('claw: post-job')
                                          ? TextButton(
                                              onPressed: () => _revertCommit(hash),
                                              child: const Text('Revert'),
                                            )
                                          : null,
                                    );
                                  },
                                ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  IconData _eventIcon(String type) {
    switch (type) {
      case 'initialized': return Icons.play_arrow;
      case 'forked': return Icons.fork_right;
      case 'job_started': return Icons.rocket_launch;
      case 'job_completed': return Icons.check_circle;
      case 'job_failed': return Icons.error;
      case 'snapshot_promoted': return Icons.arrow_upward;
      case 'file_modified': return Icons.edit;
      case 'synced': return Icons.sync;
      case 'reverted': return Icons.undo;
      case 'child_forked': return Icons.account_tree;
      default: return Icons.info;
    }
  }

  Color _eventColor(String type) {
    switch (type) {
      case 'job_completed': return Colors.green;
      case 'job_failed': return Colors.red;
      case 'job_started': return Colors.blue;
      case 'initialized': return Colors.teal;
      case 'forked': case 'child_forked': return Colors.purple;
      default: return Colors.grey;
    }
  }

  Future<void> _syncWorkspace() async {
    try {
      await ref.read(apiClientProvider).syncWorkspace(widget.workspaceId);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Workspace synced from remote')),
        );
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Sync failed: $e')),
        );
      }
    }
  }

  Future<void> _revertCommit(String hash) async {
    final confirm = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Revert Commit'),
        content: Text('Revert commit ${hash.substring(0, 7)}? This will undo the changes from that job.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Revert')),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).revertWorkspaceCommit(widget.workspaceId, hash);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Commit reverted')),
        );
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Revert failed: $e')),
        );
      }
    }
  }

  Future<void> _uploadFileToFolder(String? folderPath) async {
    final picked = await pickFile(accept: 'text/*');
    if (picked == null) return;

    String content;
    try {
      content = String.fromCharCodes(picked.bytes);
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Only text files can be uploaded this way. Use Upload ZIP for binary files.')),
        );
      }
      return;
    }

    final targetPath = folderPath != null ? '$folderPath/${picked.name}' : picked.name;
    try {
      await ref.read(apiClientProvider).putWorkspaceFile(widget.workspaceId, targetPath, content);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Uploaded ${picked.name}')),
        );
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Upload failed: $e')));
      }
    }
  }

  Future<void> _confirmDelete(String path, bool isDir) async {
    final confirmed = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(isDir ? 'Delete Folder' : 'Delete File'),
        content: Text(
          isDir
              ? 'Delete folder "$path" and all its contents? This cannot be undone.'
              : 'Delete file "$path"? This cannot be undone.',
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(
            style: FilledButton.styleFrom(backgroundColor: Colors.red),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    try {
      await ref.read(apiClientProvider).deleteWorkspaceFile(widget.workspaceId, path);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Deleted $path')));
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _uploadZip({String? folderPath}) async {
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

    // Optional: ask for subdirectory prefix (pre-filled from selected folder)
    final prefixCtrl = TextEditingController(text: folderPath ?? '');
    final shouldUpload = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Upload ${file.name}'),
        content: SizedBox(
          width: 400,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text('${(bytes.length / 1024).toStringAsFixed(1)} KB'),
              const SizedBox(height: 12),
              TextField(
                controller: prefixCtrl,
                decoration: const InputDecoration(
                  labelText: 'Subdirectory prefix (optional)',
                  hintText: 'e.g. .claude/skills/my-skill',
                  helperText: 'Leave empty to extract to workspace root',
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Upload')),
        ],
      ),
    );

    if (shouldUpload != true) return;

    try {
      final api = ref.read(apiClientProvider);
      final uploadResult = await api.uploadWorkspaceZip(
        widget.workspaceId,
        Uint8List.fromList(bytes),
        prefix: prefixCtrl.text.trim().isEmpty ? null : prefixCtrl.text.trim(),
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Uploaded ${uploadResult['uploaded']} files'),
        ));
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Upload failed: $e')));
      }
    }
  }

  Future<void> _showNewFileDialog({String? initialPath}) async {
    final pathCtrl = TextEditingController(
      text: initialPath != null ? '$initialPath/' : '',
    );
    final contentCtrl = TextEditingController();
    String? errorText;

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('New File'),
          content: SizedBox(
            width: 500,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: pathCtrl,
                  decoration: const InputDecoration(
                    labelText: 'File Path',
                    hintText: 'e.g. .claude/skills/my-skill/SKILL.md',
                    helperText: 'Relative to workspace root',
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: contentCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Content',
                    alignLabelWithHint: true,
                    border: OutlineInputBorder(),
                  ),
                  maxLines: 10,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                ),
                if (errorText != null) ...[
                  const SizedBox(height: 8),
                  Text(errorText!, style: const TextStyle(color: Colors.red)),
                ],
              ],
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
            FilledButton(
              onPressed: () async {
                if (pathCtrl.text.trim().isEmpty) {
                  setDialogState(() => errorText = 'File path is required');
                  return;
                }
                try {
                  final api = ref.read(apiClientProvider);
                  await api.putWorkspaceFile(
                    widget.workspaceId, pathCtrl.text.trim(), contentCtrl.text,
                  );
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

  Future<void> _showNewFolderDialog({String? parentPath}) async {
    final pathCtrl = TextEditingController(text: parentPath != null ? '$parentPath/' : '');
    String? errorText;

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('New Folder'),
          content: SizedBox(
            width: 400,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: pathCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Folder Path',
                    hintText: 'e.g. .claude/skills/my-skill',
                    helperText: 'Relative to workspace root',
                  ),
                ),
                if (errorText != null) ...[
                  const SizedBox(height: 8),
                  Text(errorText!, style: const TextStyle(color: Colors.red)),
                ],
              ],
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
            FilledButton(
              onPressed: () async {
                if (pathCtrl.text.trim().isEmpty) {
                  setDialogState(() => errorText = 'Folder path is required');
                  return;
                }
                try {
                  // Create a .gitkeep file to establish the folder
                  final api = ref.read(apiClientProvider);
                  await api.putWorkspaceFile(
                    widget.workspaceId, '${pathCtrl.text.trim()}/.gitkeep', '',
                  );
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

  Future<void> _showFileEditor(String filePath) async {
    String content = '';
    try {
      content = await ref.read(apiClientProvider).getWorkspaceFile(
          widget.workspaceId, filePath);
    } catch (_) {}

    final contentCtrl = TextEditingController(text: content);
    String? errorText;

    if (!mounted) return;

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: Text(filePath),
          content: SizedBox(
            width: 600,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: contentCtrl,
                  maxLines: 16,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                  decoration: const InputDecoration(
                    border: OutlineInputBorder(),
                    alignLabelWithHint: true,
                  ),
                ),
                if (errorText != null) ...[
                  const SizedBox(height: 8),
                  Text(errorText!, style: const TextStyle(color: Colors.red)),
                ],
              ],
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
            FilledButton(
              onPressed: () async {
                try {
                  final api = ref.read(apiClientProvider);
                  await api.putWorkspaceFile(
                    widget.workspaceId, filePath, contentCtrl.text,
                  );
                  if (ctx.mounted) Navigator.pop(ctx, true);
                } catch (e) {
                  setDialogState(() => errorText = 'Failed: $e');
                }
              },
              child: const Text('Save'),
            ),
          ],
        ),
      ),
    );
    if (saved == true) _refresh();
  }

}
