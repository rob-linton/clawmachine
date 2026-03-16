import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';

class PipelinesScreen extends ConsumerStatefulWidget {
  const PipelinesScreen({super.key});

  @override
  ConsumerState<PipelinesScreen> createState() => _PipelinesScreenState();
}

class _PipelinesScreenState extends ConsumerState<PipelinesScreen> {
  List<dynamic> _pipelines = [];
  List<dynamic> _runs = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final api = ref.read(apiClientProvider);
      final pResp = await api.listPipelines();
      final rResp = await api.listPipelineRuns();
      setState(() {
        _pipelines = pResp;
        _runs = rResp;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _showCreateDialog() async {
    final nameCtrl = TextEditingController();
    final descCtrl = TextEditingController();
    final steps = <Map<String, TextEditingController>>[];
    final stepModels = <String?>[]; // model per step

    void addStep() {
      steps.add({
        'name': TextEditingController(),
        'prompt': TextEditingController(),
      });
      stepModels.add(null);
    }

    addStep(); // Start with one step
    String? errorText;

    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: const Text('New Pipeline'),
          content: SizedBox(
            width: 600,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextField(
                    controller: nameCtrl,
                    decoration: const InputDecoration(labelText: 'Pipeline Name'),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: descCtrl,
                    decoration: const InputDecoration(labelText: 'Description'),
                  ),
                  const SizedBox(height: 16),
                  const Text('Steps', style: TextStyle(fontWeight: FontWeight.bold)),
                  const SizedBox(height: 8),
                  ...steps.asMap().entries.map((entry) {
                    final i = entry.key;
                    final s = entry.value;
                    return Card(
                      child: Padding(
                        padding: const EdgeInsets.all(12),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Row(
                              children: [
                                Text('Step ${i + 1}',
                                    style: const TextStyle(fontWeight: FontWeight.bold)),
                                const Spacer(),
                                if (steps.length > 1)
                                  IconButton(
                                    icon: const Icon(Icons.delete, size: 18),
                                    onPressed: () =>
                                        setDialogState(() { steps.removeAt(i); stepModels.removeAt(i); }),
                                  ),
                              ],
                            ),
                            TextField(
                              controller: s['name'],
                              decoration: const InputDecoration(
                                  labelText: 'Step Name', isDense: true),
                            ),
                            const SizedBox(height: 8),
                            TextField(
                              controller: s['prompt'],
                              decoration: InputDecoration(
                                labelText: 'Prompt',
                                helperText: i > 0
                                    ? 'Use {{previous_result}} to inject previous step output'
                                    : null,
                                isDense: true,
                              ),
                              maxLines: 3,
                              style: const TextStyle(
                                  fontFamily: 'monospace', fontSize: 13),
                            ),
                            const SizedBox(height: 8),
                            DropdownButtonFormField<String?>(
                              value: stepModels[i],
                              decoration: const InputDecoration(
                                  labelText: 'Model', isDense: true),
                              items: const [
                                DropdownMenuItem(value: null, child: Text('Default')),
                                DropdownMenuItem(value: 'sonnet', child: Text('Sonnet')),
                                DropdownMenuItem(value: 'opus', child: Text('Opus')),
                                DropdownMenuItem(value: 'haiku', child: Text('Haiku')),
                              ],
                              onChanged: (v) => setDialogState(() => stepModels[i] = v),
                            ),
                          ],
                        ),
                      ),
                    );
                  }),
                  const SizedBox(height: 8),
                  OutlinedButton.icon(
                    onPressed: () => setDialogState(() => addStep()),
                    icon: const Icon(Icons.add),
                    label: const Text('Add Step'),
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
                if (steps.any((s) => s['prompt']!.text.trim().isEmpty)) {
                  setDialogState(() => errorText = 'All steps need a prompt');
                  return;
                }
                try {
                  await ref.read(apiClientProvider).createPipeline({
                    'name': nameCtrl.text.trim(),
                    'description': descCtrl.text.trim(),
                    'steps': steps
                        .asMap()
                        .entries
                        .map((entry) => <String, dynamic>{
                              'name': entry.value['name']!.text.trim().isEmpty
                                  ? 'Step ${entry.key + 1}'
                                  : entry.value['name']!.text.trim(),
                              'prompt': entry.value['prompt']!.text.trim(),
                              if (stepModels[entry.key] != null)
                                'model': stepModels[entry.key],
                            })
                        .toList(),
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

  Future<void> _runPipeline(String id, String name) async {
    try {
      final result = await ref.read(apiClientProvider).runPipeline(id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Pipeline "$name" started (run: ${result['run_id']})'),
        ));
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed: $e')));
      }
    }
  }

  Future<void> _deletePipeline(String id) async {
    try {
      await ref.read(apiClientProvider).deletePipeline(id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
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
                label: 'Pipelines',
                child: Text('Pipelines',
                    style: Theme.of(context).textTheme.headlineMedium),
              ),
              const Spacer(),
              FilledButton.icon(
                onPressed: _showCreateDialog,
                icon: const Icon(Icons.add),
                label: const Text('New Pipeline'),
              ),
              const SizedBox(width: 8),
              IconButton(onPressed: _refresh, icon: const Icon(Icons.refresh)),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else ...[
            // Pipeline templates
            if (_pipelines.isEmpty)
              const Center(
                child: Padding(
                  padding: EdgeInsets.all(24),
                  child: Text('No pipelines yet. Create one to chain multiple jobs.'),
                ),
              )
            else
              ..._pipelines.map((p) => Card(
                    child: ListTile(
                      leading: const Icon(Icons.linear_scale),
                      title: Text(p['name'] ?? '',
                          style: const TextStyle(fontWeight: FontWeight.bold)),
                      subtitle: Text(
                          '${(p['steps'] as List?)?.length ?? 0} steps'
                          '${p['description'] != null && p['description'] != '' ? ' — ${p['description']}' : ''}'),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          FilledButton(
                            onPressed: () =>
                                _runPipeline(p['id'], p['name'] ?? ''),
                            child: const Text('Run'),
                          ),
                          const SizedBox(width: 8),
                          IconButton(
                            icon: const Icon(Icons.delete, size: 18),
                            onPressed: () => _deletePipeline(p['id']),
                          ),
                        ],
                      ),
                    ),
                  )),

            // Recent runs
            if (_runs.isNotEmpty) ...[
              const SizedBox(height: 24),
              Text('Recent Runs',
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              ..._runs.take(10).map((r) {
                final status = r['status'] ?? 'unknown';
                final icon = status == 'completed'
                    ? Icons.check_circle
                    : status == 'failed'
                        ? Icons.error
                        : Icons.play_circle;
                final color = status == 'completed'
                    ? Colors.green
                    : status == 'failed'
                        ? Colors.red
                        : Colors.orange;
                final stepJobs = (r['step_jobs'] as List?) ?? [];
                return ExpansionTile(
                  leading: Icon(icon, color: color, size: 20),
                  title: Text(r['pipeline_name'] ?? ''),
                  subtitle: Text(
                      'Step ${(r['current_step'] ?? 0) + 1}/${stepJobs.length} — $status'
                      '${r['error'] != null ? ' — ${r['error']}' : ''}'),
                  children: [
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 8),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          ...stepJobs.asMap().entries.map((entry) {
                            final idx = entry.key;
                            final jobId = entry.value;
                            if (jobId == null) {
                              return ListTile(
                                dense: true,
                                leading: const Icon(Icons.pending, size: 16, color: Colors.grey),
                                title: Text('Step ${idx + 1}: pending'),
                              );
                            }
                            final shortId = jobId.toString().length >= 8
                                ? jobId.toString().substring(0, 8)
                                : jobId.toString();
                            return ListTile(
                              dense: true,
                              leading: Icon(
                                idx <= (r['current_step'] ?? 0) ? Icons.check : Icons.pending,
                                size: 16,
                                color: idx <= (r['current_step'] ?? 0) ? Colors.green : Colors.grey,
                              ),
                              title: Text('Step ${idx + 1}: $shortId'),
                              trailing: const Icon(Icons.open_in_new, size: 14),
                              onTap: () => context.go('/jobs/$jobId'),
                            );
                          }),
                        ],
                      ),
                    ),
                  ],
                );
              }),
            ],
          ],
        ],
      ),
    );
  }
}
