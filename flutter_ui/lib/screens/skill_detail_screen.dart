import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:web/web.dart' as web;
import '../main.dart';
import '../models/skill.dart';

class SkillDetailScreen extends ConsumerStatefulWidget {
  final String skillId;
  const SkillDetailScreen({super.key, required this.skillId});

  @override
  ConsumerState<SkillDetailScreen> createState() => _SkillDetailScreenState();
}

class _SkillDetailScreenState extends ConsumerState<SkillDetailScreen> {
  Skill? _skill;
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
      final skill = await ref.read(apiClientProvider).getSkill(widget.skillId);
      setState(() {
        _skill = skill;
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
    final skill = _skill;
    if (skill == null) return;
    final updated = Skill(
      id: skill.id,
      name: skill.name,
      content: skill.content,
      description: skill.description,
      tags: skill.tags,
      files: skill.files,
      version: skill.version,
      author: skill.author,
      license: skill.license,
      sourceUrl: skill.sourceUrl,
      enabled: !skill.enabled,
    );
    try {
      await ref.read(apiClientProvider).updateSkill(skill.id, updated);
      setState(() => _skill = updated);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to update: $e')));
      }
    }
  }

  void _exportSkill() {
    final skill = _skill;
    if (skill == null) return;
    final url = ref.read(apiClientProvider).skillDownloadUrl(skill.id);
    final anchor = web.document.createElement('a') as web.HTMLAnchorElement;
    anchor.href = url;
    anchor.download = '${skill.id}.zip';
    anchor.click();
  }

  Future<void> _deleteSkill() async {
    final confirm = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Skill'),
        content: const Text('Are you sure? This cannot be undone.'),
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
      await ref.read(apiClientProvider).deleteSkill(widget.skillId);
      if (mounted) context.go('/skills');
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _showEditDialog() async {
    final skill = _skill;
    if (skill == null) return;

    final nameCtrl = TextEditingController(text: skill.name);
    final descCtrl = TextEditingController(text: skill.description);
    final contentCtrl = TextEditingController(text: skill.content);
    final tagsCtrl = TextEditingController(text: skill.tags.join(', '));
    final versionCtrl = TextEditingController(text: skill.version);
    final authorCtrl = TextEditingController(text: skill.author);
    final licenseCtrl = TextEditingController(text: skill.license ?? '');
    final sourceUrlCtrl = TextEditingController(text: skill.sourceUrl ?? '');

    final saved = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Semantics(
          header: true,
          label: 'Edit Skill',
          child: const Text('Edit Skill'),
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
                      labelText: 'Description', border: OutlineInputBorder()),
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
                  controller: licenseCtrl,
                  decoration: const InputDecoration(
                      labelText: 'License', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: sourceUrlCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Source URL', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: tagsCtrl,
                  decoration: const InputDecoration(
                      labelText: 'Tags (comma-separated)',
                      border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: contentCtrl,
                  maxLines: 10,
                  decoration: const InputDecoration(
                      labelText: 'SKILL.md Content',
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
              final updated = Skill(
                id: skill.id,
                name: nameCtrl.text.trim(),
                content: contentCtrl.text,
                description: descCtrl.text.trim(),
                tags: tags,
                files: skill.files,
                version: versionCtrl.text.trim(),
                author: authorCtrl.text.trim(),
                license: licenseCtrl.text.trim().isEmpty
                    ? null
                    : licenseCtrl.text.trim(),
                sourceUrl: sourceUrlCtrl.text.trim().isEmpty
                    ? null
                    : sourceUrlCtrl.text.trim(),
                enabled: skill.enabled,
              );
              try {
                await ref
                    .read(apiClientProvider)
                    .updateSkill(skill.id, updated);
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
    if (_error != null || _skill == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(_error ?? 'Skill not found'),
            const SizedBox(height: 16),
            FilledButton(
                onPressed: () => context.go('/skills'),
                child: const Text('Back to Skills')),
          ],
        ),
      );
    }

    final skill = _skill!;

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
                  onPressed: () => context.go('/skills'),
                  tooltip: 'Back to Skills',
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Semantics(
                    header: true,
                    label: skill.name,
                    child: Text(skill.name,
                        style: Theme.of(context).textTheme.headlineMedium),
                  ),
                ),
                if (skill.version.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(left: 8),
                    child: Semantics(
                      label: 'Version ${skill.version}',
                      child: Chip(label: Text('v${skill.version}')),
                    ),
                  ),
                const SizedBox(width: 16),
                FilledButton.icon(
                  onPressed: _showEditDialog,
                  icon: const Icon(Icons.edit, size: 18),
                  label: const Text('Edit'),
                ),
                const SizedBox(width: 8),
                OutlinedButton.icon(
                  onPressed: _exportSkill,
                  icon: const Icon(Icons.download, size: 18),
                  label: const Text('Export'),
                ),
                const SizedBox(width: 8),
                Semantics(
                  label: skill.enabled ? 'Enabled' : 'Disabled',
                  child: Switch(
                    value: skill.enabled,
                    onChanged: (_) => _toggleEnabled(),
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  icon: const Icon(Icons.delete_outline),
                  onPressed: _deleteSkill,
                  tooltip: 'Delete',
                  color: Colors.red,
                ),
              ],
            ),
            if (skill.author.isNotEmpty || skill.license != null)
              Padding(
                padding: const EdgeInsets.only(left: 56, top: 4),
                child: Semantics(
                  label: [
                    if (skill.author.isNotEmpty) 'by ${skill.author}',
                    if (skill.license != null) skill.license!,
                  ].join(' \u2022 '),
                  child: Text(
                    [
                      if (skill.author.isNotEmpty) 'by ${skill.author}',
                      if (skill.license != null) skill.license!,
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
                final leftColumn = _buildLeftColumn(skill);
                final rightSidebar = _buildRightSidebar(
                    skill, skill.createdAt, skill.updatedAt);

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

  Widget _buildLeftColumn(Skill skill) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (skill.description.isNotEmpty)
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
                    label: skill.description,
                    child: Text(skill.description),
                  ),
                ],
              ),
            ),
          ),
        const SizedBox(height: 16),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Semantics(
                  header: true,
                  label: 'SKILL.md',
                  child: Text('SKILL.md',
                      style: Theme.of(context).textTheme.titleSmall),
                ),
                const SizedBox(height: 8),
                Container(
                  width: double.infinity,
                  constraints: const BoxConstraints(maxHeight: 500),
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: const Color(0xFF1E1E1E),
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: SingleChildScrollView(
                    child: Semantics(
                      label: 'Skill content',
                      child: SelectableText(
                        skill.content.isEmpty ? '(empty)' : skill.content,
                        style: const TextStyle(
                            fontFamily: 'monospace', fontSize: 13),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        if (skill.files.isNotEmpty) ...[
          const SizedBox(height: 16),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Semantics(
                    header: true,
                    label: 'Bundled Files ${skill.files.length}',
                    child: Text(
                        'Bundled Files (${skill.files.length})',
                        style: Theme.of(context).textTheme.titleSmall),
                  ),
                  const SizedBox(height: 8),
                  ...skill.files.keys.map((path) => Padding(
                        padding: const EdgeInsets.symmetric(vertical: 2),
                        child: Row(
                          children: [
                            const Icon(Icons.insert_drive_file,
                                size: 16, color: Colors.grey),
                            const SizedBox(width: 8),
                            Semantics(
                              label: 'File $path',
                              child: Text(path,
                                  style: const TextStyle(
                                      fontFamily: 'monospace', fontSize: 13)),
                            ),
                          ],
                        ),
                      )),
                ],
              ),
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildRightSidebar(
      Skill skill, String? createdAt, String? updatedAt) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _sidebarRow('ID', skill.id, mono: true),
            _sidebarRow('Version',
                skill.version.isEmpty ? 'N/A' : skill.version),
            _sidebarRow(
                'Author', skill.author.isEmpty ? 'N/A' : skill.author),
            _sidebarRow('License', skill.license ?? 'N/A'),
            if (skill.sourceUrl != null && skill.sourceUrl!.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Source URL',
                        style: TextStyle(
                            fontSize: 11, color: Colors.grey[500])),
                    Semantics(
                      label: 'Source URL ${skill.sourceUrl}',
                      child: InkWell(
                        onTap: () {},
                        child: Text(skill.sourceUrl!,
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
            if (skill.tags.isNotEmpty) ...[
              Semantics(
                header: true,
                label: 'Tags',
                child: Text('Tags',
                    style: TextStyle(fontSize: 11, color: Colors.grey[500])),
              ),
              const SizedBox(height: 4),
              Wrap(
                spacing: 4,
                runSpacing: 4,
                children: skill.tags
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
              label: skill.enabled ? 'Status: Enabled' : 'Status: Disabled',
              child: Chip(
                label: Text(skill.enabled ? 'Enabled' : 'Disabled'),
                backgroundColor:
                    skill.enabled ? Colors.green.shade900 : Colors.red.shade900,
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
