import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:web/web.dart' as web;
import '../main.dart';
import '../models/skill.dart';
import '../services/file_upload.dart';

class SkillsScreen extends ConsumerStatefulWidget {
  const SkillsScreen({super.key});

  @override
  ConsumerState<SkillsScreen> createState() => _SkillsScreenState();
}

class _SkillsScreenState extends ConsumerState<SkillsScreen> {
  List<Skill> _skills = [];
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
      final skills = await ref.read(apiClientProvider).listSkills();
      // Fetch catalog recommended items
      List<Map<String, dynamic>> recommended = [];
      try {
        final catalog = await ref.read(apiClientProvider).fetchCatalog();
        final catalogItems = List<Map<String, dynamic>>.from(catalog['skills'] ?? []);
        final installedIds = skills.map((s) => s.id).toSet();
        recommended = catalogItems.where((item) => !installedIds.contains(item['id'])).toList();
      } catch (_) {
        // Catalog fetch failure is non-fatal
      }
      setState(() {
        _skills = skills;
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
      await ref.read(apiClientProvider).installSkillFromUrl(url: url, path: path);
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
        title: const Text('Delete Skill'),
        content: const Text('Are you sure?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Delete')),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).deleteSkill(id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  void _exportSkill(Skill skill) {
    final url = ref.read(apiClientProvider).skillDownloadUrl(skill.id);
    final anchor = web.document.createElement('a') as web.HTMLAnchorElement;
    anchor.href = url;
    anchor.download = '${skill.id}.zip';
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
            label: 'Install Skill from URL',
            child: const Text('Install Skill from URL'),
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
                      hintText: 'skills/my-skill',
                      helperText: 'Path within the repo to look for SKILL.md + manifest.json',
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
                        await ref.read(apiClientProvider).installSkillFromUrl(
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

  Future<void> _importSkillZip() async {
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
                    decoration: const InputDecoration(labelText: 'Skill ID'),
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
                  await ref.read(apiClientProvider).uploadSkillZip(
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

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Semantics(header: true, label: 'Skills', child: Text('Skills',
                  style: Theme.of(context).textTheme.headlineMedium)),
              const Spacer(),
              FilledButton.icon(
                onPressed: _installFromUrl,
                icon: const Icon(Icons.link),
                label: const Text('Install from URL'),
              ),
              const SizedBox(width: 8),
              FilledButton.icon(
                onPressed: _importSkillZip,
                icon: const Icon(Icons.archive),
                label: const Text('Import Skill (ZIP)'),
              ),
              const SizedBox(width: 8),
              IconButton(
                  onPressed: _refresh, icon: const Icon(Icons.refresh)),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Expanded(
                child: Center(child: CircularProgressIndicator()))
          else if (_skills.isEmpty && _recommended.isEmpty)
            const Expanded(child: Center(child: Text('No skills created yet.')))
          else
            Expanded(
              child: ListView(
                children: [
                  if (_skills.isNotEmpty)
                    GridView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                        maxCrossAxisExtent: 350,
                        mainAxisSpacing: 12,
                        crossAxisSpacing: 12,
                        childAspectRatio: 1.6,
                      ),
                      itemCount: _skills.length,
                      itemBuilder: (context, i) {
                        final skill = _skills[i];
                        return Opacity(
                          opacity: skill.enabled ? 1.0 : 0.5,
                          child: Card(
                            clipBehavior: Clip.antiAlias,
                            child: Semantics(
                              label: 'Skill ${skill.name}',
                              child: InkWell(
                              onTap: () => context.go('/skills/${skill.id}'),
                              child: Padding(
                                padding: const EdgeInsets.all(16),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Row(
                                      children: [
                                        Expanded(
                                          child: Semantics(label: 'Skill ${skill.name}', child: Text(skill.name,
                                              style: Theme.of(context)
                                                  .textTheme
                                                  .titleMedium)),
                                        ),
                                        PopupMenuButton<String>(
                                          itemBuilder: (_) => [
                                            const PopupMenuItem(value: 'edit', child: Text('Edit')),
                                            const PopupMenuItem(value: 'export', child: Text('Export')),
                                            const PopupMenuItem(value: 'delete', child: Text('Delete')),
                                          ],
                                          onSelected: (v) {
                                            if (v == 'edit') context.go('/skills/${skill.id}');
                                            if (v == 'export') _exportSkill(skill);
                                            if (v == 'delete') _delete(skill.id);
                                          },
                                        ),
                                      ],
                                    ),
                                    if (skill.version.isNotEmpty || skill.author.isNotEmpty)
                                      Semantics(
                                        label: [
                                          if (skill.version.isNotEmpty) 'v${skill.version}',
                                          if (skill.author.isNotEmpty) 'by ${skill.author}',
                                        ].join(' '),
                                        child: Text(
                                          [
                                            if (skill.version.isNotEmpty) 'v${skill.version}',
                                            if (skill.author.isNotEmpty) 'by ${skill.author}',
                                          ].join(' '),
                                          style: const TextStyle(fontSize: 11, color: Colors.grey),
                                        ),
                                      ),
                                    if (!skill.enabled)
                                      Semantics(
                                        label: 'Disabled',
                                        child: Chip(
                                          label: const Text('Disabled', style: TextStyle(fontSize: 10)),
                                          backgroundColor: Colors.red.shade900,
                                          visualDensity: VisualDensity.compact,
                                          padding: EdgeInsets.zero,
                                        ),
                                      ),
                                    if (skill.files.isNotEmpty)
                                      Text('${skill.files.length} files',
                                          style: const TextStyle(fontSize: 11, color: Colors.grey)),
                                    const SizedBox(height: 4),
                                    if (skill.description.isNotEmpty)
                                      Text(skill.description,
                                          maxLines: 2,
                                          overflow: TextOverflow.ellipsis,
                                          style: Theme.of(context)
                                              .textTheme
                                              .bodySmall),
                                    const Spacer(),
                                    if (skill.tags.isNotEmpty)
                                      Wrap(
                                        spacing: 4,
                                        children: skill.tags
                                            .map((t) => Chip(
                                                  label: Text(t,
                                                      style: const TextStyle(
                                                          fontSize: 10)),
                                                  visualDensity:
                                                      VisualDensity.compact,
                                                  padding: EdgeInsets.zero,
                                                ))
                                            .toList(),
                                      ),
                                  ],
                                ),
                              ),
                            ),
                          ),
                        ));
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
                        leading: const Icon(Icons.auto_awesome),
                        title: Semantics(
                          label: 'Recommended skill ${item['name'] ?? item['id']}',
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
    );
  }
}
