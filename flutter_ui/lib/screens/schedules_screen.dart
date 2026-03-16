import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';
import '../models/cron_schedule.dart';
import '../models/skill.dart';
import '../models/workspace.dart';
import '../widgets/skill_selector.dart';

class SchedulesScreen extends ConsumerStatefulWidget {
  const SchedulesScreen({super.key});

  @override
  ConsumerState<SchedulesScreen> createState() => _SchedulesScreenState();
}

class _SchedulesScreenState extends ConsumerState<SchedulesScreen> {
  List<CronSchedule> _crons = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final crons = await ref.read(apiClientProvider).listCrons();
      setState(() {
        _crons = crons;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to load schedules: $e')));
      }
    }
  }

  Future<void> _triggerCron(CronSchedule cron) async {
    try {
      final result = await ref.read(apiClientProvider).triggerCron(cron.id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Triggered! Job ${result['job_id']}'),
        ));
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Trigger failed: $e')));
      }
    }
  }

  Future<void> _deleteCron(CronSchedule cron) async {
    final confirm = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Schedule'),
        content: Text('Delete "${cron.name}"?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Delete')),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).deleteCron(cron.id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _showCreateEditDialog({CronSchedule? existing}) async {
    final nameCtrl = TextEditingController(text: existing?.name ?? '');
    final scheduleCtrl = TextEditingController(text: existing?.schedule ?? '');
    final promptCtrl = TextEditingController(text: existing?.prompt ?? '');
    final workingDirCtrl = TextEditingController(
        text: existing != null && existing.workingDir != '.' ? existing.workingDir : '');
    String? model = existing?.model;
    int priority = existing?.priority ?? 5;
    bool enabled = existing?.enabled ?? true;
    String? selectedTemplateId;
    String? workspaceId;
    final selectedSkills = <String>{...existing?.skillIds ?? []};
    final budgetCtrl = TextEditingController(text: existing?.maxBudgetUsd?.toString() ?? '');
    final tagsCtrl = TextEditingController(text: existing?.tags.join(', ') ?? '');
    String outputType = 'redis';
    final outputPathCtrl = TextEditingController();
    final webhookUrlCtrl = TextEditingController();
    List<dynamic> templates = [];
    List<Skill> skills = [];
    List<Workspace> workspaces = [];
    try { templates = await ref.read(apiClientProvider).listJobTemplates(); } catch (_) {}
    try { skills = await ref.read(apiClientProvider).listSkills(); } catch (_) {}
    try { workspaces = await ref.read(apiClientProvider).listWorkspaces(); } catch (_) {}
    String? errorText;

    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) => AlertDialog(
          title: Text(existing != null ? 'Edit Schedule' : 'New Schedule'),
          content: SizedBox(
            width: 500,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextField(
                    controller: nameCtrl,
                    decoration: const InputDecoration(labelText: 'Name'),
                  ),
                  const SizedBox(height: 12),
                  if (templates.isNotEmpty)
                    DropdownButtonFormField<String?>(
                      value: selectedTemplateId,
                      decoration: const InputDecoration(
                        labelText: 'Template (optional)',
                        helperText: 'Use a template instead of inline prompt',
                      ),
                      items: [
                        const DropdownMenuItem(value: null, child: Text('None (inline)')),
                        ...templates.map((t) => DropdownMenuItem(
                              value: t['id'] as String?,
                              child: Text(t['name'] ?? ''),
                            )),
                      ],
                      onChanged: (id) {
                        setDialogState(() {
                          selectedTemplateId = id;
                          if (id != null) {
                            final tmpl = templates.firstWhere((t) => t['id'] == id, orElse: () => null);
                            if (tmpl != null) {
                              promptCtrl.text = tmpl['prompt'] ?? '';
                              model = tmpl['model'];
                            }
                          }
                        });
                      },
                    ),
                  if (templates.isNotEmpty) const SizedBox(height: 12),
                  TextField(
                    controller: scheduleCtrl,
                    decoration: InputDecoration(
                      labelText: 'Cron Expression',
                      helperText: 'sec min hour day month weekday (e.g. 0 */5 * * * *)',
                      errorText: errorText,
                    ),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: promptCtrl,
                    decoration: const InputDecoration(labelText: 'Prompt'),
                    maxLines: 4,
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: workingDirCtrl,
                    decoration: const InputDecoration(
                      labelText: 'Working Directory (optional)',
                    ),
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
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      const Text('Priority: '),
                      Expanded(
                        child: Slider(
                          value: priority.toDouble(),
                          min: 0,
                          max: 9,
                          divisions: 9,
                          label: priority.toString(),
                          onChanged: (v) =>
                              setDialogState(() => priority = v.round()),
                        ),
                      ),
                      Text('$priority'),
                    ],
                  ),
                  // Workspace
                  if (workspaces.isNotEmpty) ...[
                    const SizedBox(height: 12),
                    DropdownButtonFormField<String?>(
                      value: workspaceId,
                      decoration: const InputDecoration(labelText: 'Workspace'),
                      items: [
                        const DropdownMenuItem(value: null, child: Text('None')),
                        ...workspaces.map((w) => DropdownMenuItem(value: w.id, child: Text(w.name))),
                      ],
                      onChanged: (v) => setDialogState(() => workspaceId = v),
                    ),
                  ],

                  // Skills
                  const SizedBox(height: 12),
                  SkillSelector(
                    availableSkills: skills,
                    selectedIds: selectedSkills,
                    onChanged: (ids) => setDialogState(() {
                      selectedSkills.clear();
                      selectedSkills.addAll(ids);
                    }),
                  ),

                  // Advanced: budget, output, tags
                  ExpansionTile(
                    title: const Text('Advanced'),
                    tilePadding: EdgeInsets.zero,
                    children: [
                      TextField(
                        controller: budgetCtrl,
                        decoration: const InputDecoration(labelText: 'Max Budget USD (approximate)'),
                        keyboardType: TextInputType.number,
                      ),
                      const SizedBox(height: 12),
                      TextField(
                        controller: tagsCtrl,
                        decoration: const InputDecoration(labelText: 'Tags (comma-separated)'),
                      ),
                      const SizedBox(height: 12),
                      const Text('Output Destination', style: TextStyle(fontSize: 13)),
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
                    ],
                  ),

                  SwitchListTile(
                    title: const Text('Enabled'),
                    value: enabled,
                    onChanged: (v) => setDialogState(() => enabled = v),
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
                if (nameCtrl.text.isEmpty || scheduleCtrl.text.isEmpty || promptCtrl.text.isEmpty) {
                  setDialogState(() => errorText = 'Name, schedule, and prompt are required');
                  return;
                }
                final data = {
                  'name': nameCtrl.text,
                  'schedule': scheduleCtrl.text,
                  'enabled': enabled,
                  'prompt': promptCtrl.text,
                  'priority': priority,
                  if (model != null) 'model': model,
                  if (workingDirCtrl.text.isNotEmpty)
                    'working_dir': workingDirCtrl.text,
                  if (selectedTemplateId != null)
                    'template_id': selectedTemplateId,
                  if (workspaceId != null) 'workspace_id': workspaceId,
                  if (selectedSkills.isNotEmpty) 'skill_ids': selectedSkills.toList(),
                  if (budgetCtrl.text.trim().isNotEmpty)
                    'max_budget_usd': double.tryParse(budgetCtrl.text.trim()),
                  if (tagsCtrl.text.trim().isNotEmpty)
                    'tags': tagsCtrl.text.split(',').map((t) => t.trim()).where((t) => t.isNotEmpty).toList(),
                  if (outputType == 'file' && outputPathCtrl.text.trim().isNotEmpty)
                    'output_dest': {'type': 'file', 'path': outputPathCtrl.text.trim()},
                  if (outputType == 'webhook' && webhookUrlCtrl.text.trim().isNotEmpty)
                    'output_dest': {'type': 'webhook', 'url': webhookUrlCtrl.text.trim()},
                };
                try {
                  final api = ref.read(apiClientProvider);
                  if (existing != null) {
                    await api.updateCron(existing.id, data);
                  } else {
                    await api.createCron(data);
                  }
                  if (ctx.mounted) Navigator.pop(ctx, true);
                } catch (e) {
                  final msg = e.toString();
                  if (msg.contains('422') || msg.contains('Invalid cron')) {
                    setDialogState(() => errorText = 'Invalid cron expression');
                  } else {
                    setDialogState(() => errorText = 'Failed: $msg');
                  }
                }
              },
              child: Text(existing != null ? 'Save' : 'Create'),
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
              Semantics(header: true, label: 'Schedules', child: Text('Schedules', style: Theme.of(context).textTheme.headlineMedium)),
              const Spacer(),
              FilledButton.icon(
                onPressed: () => _showCreateEditDialog(),
                icon: const Icon(Icons.add),
                label: const Text('New Schedule'),
              ),
              const SizedBox(width: 8),
              IconButton(onPressed: _refresh, icon: const Icon(Icons.refresh)),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else if (_crons.isEmpty)
            const Center(
              child: Padding(
                padding: EdgeInsets.all(48),
                child: Text('No schedules yet. Create one to run jobs on a cron.'),
              ),
            )
          else
            Expanded(
              child: ListView.builder(
                itemCount: _crons.length,
                itemBuilder: (context, i) => _buildCronTile(_crons[i]),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildCronTile(CronSchedule cron) {
    return Card(
      child: ListTile(
        title: Row(
          children: [
            Semantics(label: 'Schedule ${cron.name}', child: Text(cron.name, style: const TextStyle(fontWeight: FontWeight.bold))),
            const SizedBox(width: 8),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
              decoration: BoxDecoration(
                color: cron.enabled
                    ? Colors.green.withValues(alpha: 0.2)
                    : Colors.grey.withValues(alpha: 0.2),
                borderRadius: BorderRadius.circular(4),
              ),
              child: Text(
                cron.enabled ? 'enabled' : 'disabled',
                style: TextStyle(
                  fontSize: 12,
                  color: cron.enabled ? Colors.green : Colors.grey,
                ),
              ),
            ),
          ],
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 4),
            Text(cron.schedule, style: const TextStyle(fontFamily: 'monospace')),
            Text(cron.prompt.length > 80
                ? '${cron.prompt.substring(0, 80)}...'
                : cron.prompt),
            if (cron.lastRun != null) Text('Last run: ${cron.lastRun}', style: const TextStyle(fontSize: 12)),
          ],
        ),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              icon: const Icon(Icons.play_arrow),
              tooltip: 'Trigger Now',
              onPressed: () => _triggerCron(cron),
            ),
            IconButton(
              icon: const Icon(Icons.edit),
              tooltip: 'Edit',
              onPressed: () => _showCreateEditDialog(existing: cron),
            ),
            IconButton(
              icon: const Icon(Icons.delete),
              tooltip: 'Delete',
              onPressed: () => _deleteCron(cron),
            ),
          ],
        ),
      ),
    );
  }
}
