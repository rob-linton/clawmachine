import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';
import '../models/skill.dart';
import '../models/workspace.dart';

class TemplatesScreen extends ConsumerStatefulWidget {
  const TemplatesScreen({super.key});

  @override
  ConsumerState<TemplatesScreen> createState() => _TemplatesScreenState();
}

class _TemplatesScreenState extends ConsumerState<TemplatesScreen> {
  List<dynamic> _templates = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final templates = await ref.read(apiClientProvider).listJobTemplates();
      setState(() { _templates = templates; _loading = false; });
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _runTemplate(String id, String name) async {
    try {
      final result = await ref.read(apiClientProvider).runJobTemplate(id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Job started from "$name" (${result['job_id']})'),
        ));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Run failed: $e')));
      }
    }
  }

  Future<void> _deleteTemplate(String id) async {
    try {
      await ref.read(apiClientProvider).deleteJobTemplate(id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _showTemplateDialog({dynamic existing}) async {
    // Load skills and workspaces for selectors
    List<Skill> skills = [];
    List<Workspace> workspaces = [];
    try { skills = await ref.read(apiClientProvider).listSkills(); } catch (_) {}
    try { workspaces = await ref.read(apiClientProvider).listWorkspaces(); } catch (_) {}

    final isEdit = existing != null;
    final nameCtrl = TextEditingController(text: existing?['name'] ?? '');
    final descCtrl = TextEditingController(text: existing?['description'] ?? '');
    final promptCtrl = TextEditingController(text: existing?['prompt'] ?? '');
    final timeoutCtrl = TextEditingController(
        text: existing?['timeout_secs']?.toString() ?? '1800');
    final allowedToolsCtrl = TextEditingController(
        text: (existing?['allowed_tools'] as List?)?.join(', ') ?? '');
    final tagsCtrl = TextEditingController(
        text: (existing?['tags'] as List?)?.join(', ') ?? '');
    String? model = existing?['model'];
    String? workspaceId = existing?['workspace_id'];
    double priority = (existing?['priority'] ?? 5).toDouble();
    final selectedSkills = <String>{
      ...List<String>.from(existing?['skill_ids'] ?? [])
    };
    String outputType = 'redis';
    final outputPathCtrl = TextEditingController();
    final webhookUrlCtrl = TextEditingController();
    // Parse existing output_dest
    if (existing?['output_dest'] is Map) {
      final od = existing['output_dest'];
      if (od['type'] == 'file') { outputType = 'file'; outputPathCtrl.text = od['path'] ?? ''; }
      if (od['type'] == 'webhook') { outputType = 'webhook'; webhookUrlCtrl.text = od['url'] ?? ''; }
    }
    String? errorText;

    if (!mounted) return;
    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: Text(isEdit ? 'Edit Template' : 'New Template'),
          content: SizedBox(
            width: 600,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Basic fields
                  TextField(controller: nameCtrl, decoration: const InputDecoration(labelText: 'Name')),
                  const SizedBox(height: 12),
                  TextField(controller: descCtrl, decoration: const InputDecoration(labelText: 'Description')),
                  const SizedBox(height: 12),
                  TextField(
                    controller: promptCtrl,
                    decoration: const InputDecoration(labelText: 'Prompt', alignLabelWithHint: true),
                    maxLines: 5,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      Expanded(
                        child: DropdownButtonFormField<String?>(
                          value: model,
                          decoration: const InputDecoration(labelText: 'Model'),
                          items: const [
                            DropdownMenuItem(value: null, child: Text('Default')),
                            DropdownMenuItem(value: 'sonnet', child: Text('Sonnet')),
                            DropdownMenuItem(value: 'opus', child: Text('Opus')),
                            DropdownMenuItem(value: 'haiku', child: Text('Haiku')),
                          ],
                          onChanged: (v) => setDialogState(() => model = v),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: DropdownButtonFormField<String?>(
                          value: workspaceId,
                          decoration: const InputDecoration(labelText: 'Workspace'),
                          items: [
                            const DropdownMenuItem(value: null, child: Text('None')),
                            ...workspaces.map((w) => DropdownMenuItem(value: w.id, child: Text(w.name))),
                          ],
                          onChanged: (v) => setDialogState(() => workspaceId = v),
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),

                  // Skills
                  if (skills.isNotEmpty) ...[
                    const Text('Skills', style: TextStyle(fontWeight: FontWeight.bold, fontSize: 13)),
                    const SizedBox(height: 4),
                    Wrap(
                      spacing: 6,
                      children: skills.map((s) {
                        final selected = selectedSkills.contains(s.id);
                        return FilterChip(
                          label: Text(s.name),
                          selected: selected,
                          onSelected: (v) {
                            setDialogState(() {
                              if (v) selectedSkills.add(s.id); else selectedSkills.remove(s.id);
                            });
                          },
                        );
                      }).toList(),
                    ),
                    const SizedBox(height: 12),
                  ],

                  // Advanced Options
                  ExpansionTile(
                    title: const Text('Advanced Options'),
                    tilePadding: EdgeInsets.zero,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text('Priority: ${priority.round()}', style: const TextStyle(fontSize: 13)),
                                Slider(
                                  value: priority, min: 0, max: 9, divisions: 9,
                                  label: priority.round().toString(),
                                  onChanged: (v) => setDialogState(() => priority = v),
                                ),
                              ],
                            ),
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: TextField(
                              controller: timeoutCtrl,
                              decoration: const InputDecoration(labelText: 'Timeout (seconds)'),
                              keyboardType: TextInputType.number,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 12),
                      TextField(
                        controller: allowedToolsCtrl,
                        decoration: const InputDecoration(
                          labelText: 'Allowed Tools',
                          hintText: 'Read,Write,Edit,Glob,Grep,Bash',
                        ),
                      ),
                      const SizedBox(height: 12),
                      TextField(
                        controller: tagsCtrl,
                        decoration: const InputDecoration(labelText: 'Tags (comma-separated)'),
                      ),
                      const SizedBox(height: 12),
                      const Text('Output Destination', style: TextStyle(fontSize: 13)),
                      const SizedBox(height: 4),
                      SegmentedButton<String>(
                        segments: const [
                          ButtonSegment(value: 'redis', label: Text('Redis')),
                          ButtonSegment(value: 'file', label: Text('File')),
                          ButtonSegment(value: 'webhook', label: Text('Webhook')),
                        ],
                        selected: {outputType},
                        onSelectionChanged: (v) => setDialogState(() => outputType = v.first),
                      ),
                      if (outputType == 'file') ...[
                        const SizedBox(height: 8),
                        TextField(controller: outputPathCtrl, decoration: const InputDecoration(labelText: 'Output Path')),
                      ],
                      if (outputType == 'webhook') ...[
                        const SizedBox(height: 8),
                        TextField(controller: webhookUrlCtrl, decoration: const InputDecoration(labelText: 'Webhook URL')),
                      ],
                      const SizedBox(height: 8),
                    ],
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
            TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
            FilledButton(
              onPressed: () async {
                if (nameCtrl.text.trim().isEmpty || promptCtrl.text.trim().isEmpty) {
                  setDialogState(() => errorText = 'Name and prompt are required');
                  return;
                }
                Map<String, dynamic>? outputDest;
                if (outputType == 'file' && outputPathCtrl.text.trim().isNotEmpty) {
                  outputDest = {'type': 'file', 'path': outputPathCtrl.text.trim()};
                } else if (outputType == 'webhook' && webhookUrlCtrl.text.trim().isNotEmpty) {
                  outputDest = {'type': 'webhook', 'url': webhookUrlCtrl.text.trim()};
                }
                final tags = tagsCtrl.text.split(',').map((t) => t.trim()).where((t) => t.isNotEmpty).toList();
                final allowedTools = allowedToolsCtrl.text.split(',').map((t) => t.trim()).where((t) => t.isNotEmpty).toList();
                final data = <String, dynamic>{
                  'name': nameCtrl.text.trim(),
                  'description': descCtrl.text.trim(),
                  'prompt': promptCtrl.text.trim(),
                  if (model != null) 'model': model,
                  if (workspaceId != null) 'workspace_id': workspaceId,
                  if (selectedSkills.isNotEmpty) 'skill_ids': selectedSkills.toList(),
                  'priority': priority.round(),
                  if (int.tryParse(timeoutCtrl.text.trim()) != null)
                    'timeout_secs': int.parse(timeoutCtrl.text.trim()),
                  if (allowedTools.isNotEmpty) 'allowed_tools': allowedTools,
                  if (tags.isNotEmpty) 'tags': tags,
                  if (outputDest != null) 'output_dest': outputDest,
                };
                try {
                  final api = ref.read(apiClientProvider);
                  if (isEdit) {
                    await api.updateJobTemplate(existing['id'], data);
                  } else {
                    await api.createJobTemplate(data);
                  }
                  if (ctx.mounted) Navigator.pop(ctx, true);
                } catch (e) {
                  setDialogState(() => errorText = 'Failed: $e');
                }
              },
              child: Text(isEdit ? 'Save' : 'Create'),
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
                header: true, label: 'Templates',
                child: Text('Templates', style: Theme.of(context).textTheme.headlineMedium),
              ),
              const Spacer(),
              FilledButton.icon(
                onPressed: () => _showTemplateDialog(),
                icon: const Icon(Icons.add),
                label: const Text('New Template'),
              ),
              const SizedBox(width: 8),
              IconButton(onPressed: _refresh, icon: const Icon(Icons.refresh)),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else if (_templates.isEmpty)
            const Center(
              child: Padding(
                padding: EdgeInsets.all(48),
                child: Text('No templates yet. Create one to save a reusable job definition.'),
              ),
            )
          else
            Expanded(
              child: ListView.builder(
                itemCount: _templates.length,
                itemBuilder: (context, i) {
                  final t = _templates[i];
                  final name = t['name'] ?? '';
                  final desc = t['description'] ?? '';
                  final prompt = t['prompt'] ?? '';
                  final skillCount = (t['skill_ids'] as List?)?.length ?? 0;
                  final promptPreview = prompt.length > 80 ? '${prompt.substring(0, 80)}...' : prompt;
                  return Card(
                    child: ListTile(
                      onTap: () => _showTemplateDialog(existing: t),
                      title: Semantics(
                        label: 'Template $name',
                        child: Text(name, style: const TextStyle(fontWeight: FontWeight.bold)),
                      ),
                      subtitle: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          if (desc.isNotEmpty) Text(desc),
                          Text(promptPreview, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
                          if (skillCount > 0)
                            Text('$skillCount skills', style: const TextStyle(fontSize: 11, color: Colors.grey)),
                        ],
                      ),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          FilledButton(
                            onPressed: () => _runTemplate(t['id'], name),
                            child: const Text('Run Now'),
                          ),
                          const SizedBox(width: 8),
                          IconButton(
                            icon: const Icon(Icons.delete, size: 18),
                            onPressed: () => _deleteTemplate(t['id']),
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
    );
  }
}
