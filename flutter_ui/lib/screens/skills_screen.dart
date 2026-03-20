import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
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
      setState(() {
        _skills = skills;
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

  void _showSkillDetail(Skill skill) {
    showDialog(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(skill.name),
        content: SizedBox(
          width: 600,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                _infoRow('ID', skill.id),
                _infoRow('Description', skill.description),
                if (skill.version.isNotEmpty)
                  _infoRow('Version', skill.version),
                if (skill.author.isNotEmpty)
                  _infoRow('Author', skill.author),
                if (skill.tags.isNotEmpty)
                  _infoRow('Tags', skill.tags.join(', ')),
                if (skill.files.isNotEmpty)
                  _infoRow('Files', skill.files.keys.join(', ')),
                const SizedBox(height: 16),
                const Text('SKILL.md:',
                    style: TextStyle(fontWeight: FontWeight.bold)),
                const SizedBox(height: 8),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: Colors.black54,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: SelectableText(
                    skill.content.isEmpty ? '(empty)' : skill.content,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                  ),
                ),
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  Widget _infoRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
              width: 90,
              child: Text('$label:',
                  style: const TextStyle(fontWeight: FontWeight.bold))),
          Expanded(child: Text(value)),
        ],
      ),
    );
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
          else if (_skills.isEmpty)
            const Expanded(child: Center(child: Text('No skills created yet.')))
          else
            Expanded(
              child: GridView.builder(
                gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                  maxCrossAxisExtent: 350,
                  mainAxisSpacing: 12,
                  crossAxisSpacing: 12,
                  childAspectRatio: 1.6,
                ),
                itemCount: _skills.length,
                itemBuilder: (context, i) {
                  final skill = _skills[i];
                  return Card(
                    clipBehavior: Clip.antiAlias,
                    child: Semantics(
                      label: 'Skill ${skill.name}',
                      child: InkWell(
                      onTap: () => _showSkillDetail(skill),
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
                                    if (v == 'edit') _showSkillDetail(skill);
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
                  ));
                },
              ),
            ),
        ],
      ),
    );
  }
}
