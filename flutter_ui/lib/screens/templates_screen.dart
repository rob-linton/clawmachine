import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';

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
      setState(() {
        _templates = templates;
        _loading = false;
      });
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
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Run failed: $e')));
      }
    }
  }

  Future<void> _deleteTemplate(String id) async {
    try {
      await ref.read(apiClientProvider).deleteJobTemplate(id);
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
    final promptCtrl = TextEditingController();
    String? model;
    String? errorText;

    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('New Template'),
          content: SizedBox(
            width: 600,
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
                    controller: promptCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Prompt',
                      alignLabelWithHint: true,
                    ),
                    maxLines: 6,
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                  ),
                  const SizedBox(height: 12),
                  DropdownButtonFormField<String?>(
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
                if (nameCtrl.text.trim().isEmpty || promptCtrl.text.trim().isEmpty) {
                  setDialogState(() => errorText = 'Name and prompt are required');
                  return;
                }
                try {
                  await ref.read(apiClientProvider).createJobTemplate({
                    'name': nameCtrl.text.trim(),
                    'description': descCtrl.text.trim(),
                    'prompt': promptCtrl.text.trim(),
                    if (model != null) 'model': model,
                  });
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
                label: 'Templates',
                child: Text('Templates',
                    style: Theme.of(context).textTheme.headlineMedium),
              ),
              const Spacer(),
              FilledButton.icon(
                onPressed: _showCreateDialog,
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
                  final promptPreview = prompt.length > 80
                      ? '${prompt.substring(0, 80)}...'
                      : prompt;
                  return Card(
                    child: ListTile(
                      title: Semantics(
                        label: 'Template $name',
                        child: Text(name,
                            style: const TextStyle(fontWeight: FontWeight.bold)),
                      ),
                      subtitle: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          if (desc.isNotEmpty) Text(desc),
                          Text(promptPreview,
                              style: const TextStyle(
                                  fontFamily: 'monospace', fontSize: 12)),
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
