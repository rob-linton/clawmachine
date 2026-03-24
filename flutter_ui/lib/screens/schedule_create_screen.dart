import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/skill.dart';
import '../models/tool.dart';
import '../models/workspace.dart';
import '../widgets/skill_selector.dart';
import '../widgets/tool_selector.dart';

class ScheduleCreateScreen extends ConsumerStatefulWidget {
  final String? scheduleId;
  const ScheduleCreateScreen({super.key, this.scheduleId});

  @override
  ConsumerState<ScheduleCreateScreen> createState() =>
      _ScheduleCreateScreenState();
}

class _ScheduleCreateScreenState extends ConsumerState<ScheduleCreateScreen> {
  bool _loading = true;
  bool _saving = false;
  String? _error;

  final _nameCtrl = TextEditingController();
  final _scheduleCtrl = TextEditingController();
  final _promptCtrl = TextEditingController();
  final _workingDirCtrl = TextEditingController();
  final _maxBudgetCtrl = TextEditingController();
  final _tagsCtrl = TextEditingController();
  final _outputPathCtrl = TextEditingController();
  final _webhookUrlCtrl = TextEditingController();

  String? _templateId;
  String? _model;
  String? _workspaceId;
  double _priority = 5;
  bool _enabled = true;
  Set<String> _selectedSkills = {};
  Set<String> _selectedTools = {};
  String _outputType = 'redis';

  List<Skill> _skills = [];
  List<Tool> _tools = [];
  List<Workspace> _workspaces = [];
  List<dynamic> _templates = [];

  @override
  void initState() {
    super.initState();
    _loadData();
  }

  @override
  void dispose() {
    _nameCtrl.dispose();
    _scheduleCtrl.dispose();
    _promptCtrl.dispose();
    _workingDirCtrl.dispose();
    _maxBudgetCtrl.dispose();
    _tagsCtrl.dispose();
    _outputPathCtrl.dispose();
    _webhookUrlCtrl.dispose();
    super.dispose();
  }

  Future<void> _loadData() async {
    try {
      final api = ref.read(apiClientProvider);
      final results = await Future.wait([
        api.listSkills(),
        api.listTools(),
        api.listWorkspaces(),
        api.listJobTemplates(),
      ]);
      _skills = results[0] as List<Skill>;
      _tools = results[1] as List<Tool>;
      _workspaces = results[2] as List<Workspace>;
      _templates = results[3];

      if (widget.scheduleId != null) {
        final c = await api.getCron(widget.scheduleId!);
        _nameCtrl.text = c.name;
        _scheduleCtrl.text = c.schedule;
        _promptCtrl.text = c.prompt;
        _workingDirCtrl.text = c.workingDir;
        _templateId = c.templateId;
        _model = c.model;
        _workspaceId = c.workspaceId;
        _priority = c.priority.toDouble();
        _enabled = c.enabled;
        _selectedSkills = Set<String>.from(c.skillIds);
        _selectedTools = Set<String>.from(c.toolIds);
        if (c.maxBudgetUsd != null) {
          _maxBudgetCtrl.text = c.maxBudgetUsd.toString();
        }
        _tagsCtrl.text = c.tags.join(', ');
        if (c.outputDest != null) {
          final od = c.outputDest!;
          if (od['type'] == 'file') {
            _outputType = 'file';
            _outputPathCtrl.text = od['path'] ?? '';
          } else if (od['type'] == 'webhook') {
            _outputType = 'webhook';
            _webhookUrlCtrl.text = od['url'] ?? '';
          }
        }
      }

      setState(() => _loading = false);
    } catch (e) {
      setState(() {
        _error = 'Failed to load data: $e';
        _loading = false;
      });
    }
  }

  Future<void> _save() async {
    if (_nameCtrl.text.trim().isEmpty || _scheduleCtrl.text.trim().isEmpty) {
      setState(() => _error = 'Name and cron expression are required');
      return;
    }
    if (_promptCtrl.text.trim().isEmpty && _templateId == null) {
      setState(() => _error = 'A template or prompt is required');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });

    Map<String, dynamic>? outputDest;
    if (_outputType == 'file' && _outputPathCtrl.text.trim().isNotEmpty) {
      outputDest = {'type': 'file', 'path': _outputPathCtrl.text.trim()};
    } else if (_outputType == 'webhook' &&
        _webhookUrlCtrl.text.trim().isNotEmpty) {
      outputDest = {'type': 'webhook', 'url': _webhookUrlCtrl.text.trim()};
    }
    final tags = _tagsCtrl.text
        .split(',')
        .map((t) => t.trim())
        .where((t) => t.isNotEmpty)
        .toList();

    final data = <String, dynamic>{
      'name': _nameCtrl.text.trim(),
      'schedule': _scheduleCtrl.text.trim(),
      'enabled': _enabled,
      'prompt': _promptCtrl.text.trim(),
      if (_templateId != null) 'template_id': _templateId,
      if (_model != null) 'model': _model,
      if (_workspaceId != null) 'workspace_id': _workspaceId,
      if (_selectedSkills.isNotEmpty) 'skill_ids': _selectedSkills.toList(),
      if (_selectedTools.isNotEmpty) 'tool_ids': _selectedTools.toList(),
      'priority': _priority.round(),
      if (_workingDirCtrl.text.trim().isNotEmpty)
        'working_dir': _workingDirCtrl.text.trim(),
      if (double.tryParse(_maxBudgetCtrl.text.trim()) != null)
        'max_budget_usd': double.parse(_maxBudgetCtrl.text.trim()),
      if (tags.isNotEmpty) 'tags': tags,
      if (outputDest != null) 'output_dest': outputDest,
    };

    try {
      final api = ref.read(apiClientProvider);
      if (widget.scheduleId != null) {
        await api.updateCron(widget.scheduleId!, data);
      } else {
        await api.createCron(data);
      }
      if (mounted) context.go('/schedules');
    } catch (e) {
      setState(() {
        _error = 'Failed: $e';
        _saving = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final isEdit = widget.scheduleId != null;
    final title = isEdit ? 'Edit Schedule' : 'New Schedule';

    return Scaffold(
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          children: [
            Row(
              children: [
                IconButton(
                  onPressed: () => context.go('/schedules'),
                  icon: const Icon(Icons.arrow_back),
                ),
                const SizedBox(width: 8),
                Semantics(
                  header: true,
                  label: title,
                  child: Text(
                    title,
                    style: Theme.of(context).textTheme.headlineMedium,
                  ),
                ),
                const Spacer(),
                Semantics(
                  label: 'Enabled',
                  child: const Text('Enabled'),
                ),
                const SizedBox(width: 8),
                Switch(
                  value: _enabled,
                  onChanged: (v) => setState(() => _enabled = v),
                ),
                const SizedBox(width: 16),
                FilledButton(
                  onPressed: _saving ? null : _save,
                  child: Text(isEdit ? 'Save' : 'Create'),
                ),
              ],
            ),
            const SizedBox(height: 24),
            Expanded(
              child: _loading
                  ? const Center(child: CircularProgressIndicator())
                  : SingleChildScrollView(
                      child: Center(
                        child: ConstrainedBox(
                          constraints: const BoxConstraints(maxWidth: 900),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              TextField(
                                controller: _nameCtrl,
                                decoration:
                                    const InputDecoration(labelText: 'Name'),
                              ),
                              const SizedBox(height: 16),
                              TextField(
                                controller: _scheduleCtrl,
                                decoration: const InputDecoration(
                                  labelText: 'Cron Expression',
                                  helperText:
                                      'sec min hour day month weekday (e.g., 0 0 9 * * MON-FRI)',
                                ),
                                style: const TextStyle(fontFamily: 'monospace'),
                              ),
                              const SizedBox(height: 16),
                              DropdownButtonFormField<String?>(
                                value: _templateId,
                                decoration: const InputDecoration(
                                    labelText: 'Job Template'),
                                items: [
                                  const DropdownMenuItem(
                                    value: null,
                                    child: Text('None (inline prompt)'),
                                  ),
                                  ..._templates.map((t) => DropdownMenuItem(
                                        value: t['id'] as String?,
                                        child: Text(t['name'] ?? ''),
                                      )),
                                ],
                                onChanged: (id) {
                                  setState(() {
                                    _templateId = id;
                                    if (id != null) {
                                      final tmpl = _templates.cast<Map<String, dynamic>?>().firstWhere(
                                          (t) => t?['id'] == id,
                                          orElse: () => null);
                                      if (tmpl != null) {
                                        if (_promptCtrl.text.isEmpty) {
                                          _promptCtrl.text =
                                              tmpl['prompt'] ?? '';
                                        }
                                        _model ??= tmpl['model'] as String?;
                                      }
                                    }
                                  });
                                },
                              ),
                              if (_templateId != null) ...[
                                const SizedBox(height: 8),
                                Builder(builder: (_) {
                                  final tmpl = _templates.cast<Map<String, dynamic>?>().firstWhere(
                                      (t) => t?['id'] == _templateId,
                                      orElse: () => null);
                                  if (tmpl == null) return const SizedBox.shrink();
                                  final desc = tmpl['description'] ?? '';
                                  if ((desc as String).isEmpty) {
                                    return const SizedBox.shrink();
                                  }
                                  return Semantics(
                                    label: 'Template description: $desc',
                                    child: Text(
                                      desc,
                                      style: const TextStyle(
                                          fontSize: 12, color: Colors.grey),
                                    ),
                                  );
                                }),
                              ],
                              const SizedBox(height: 16),
                              TextField(
                                controller: _promptCtrl,
                                decoration: InputDecoration(
                                  labelText: _templateId != null
                                      ? 'Prompt Override (optional)'
                                      : 'Prompt',
                                  alignLabelWithHint: true,
                                ),
                                minLines: 6,
                                maxLines: null,
                                style: const TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 13,
                                ),
                              ),
                              const SizedBox(height: 16),
                              if (_workspaces.isNotEmpty)
                                DropdownButtonFormField<String?>(
                                  value: _workspaceId,
                                  decoration: const InputDecoration(
                                      labelText: 'Workspace'),
                                  items: [
                                    const DropdownMenuItem(
                                        value: null, child: Text('None')),
                                    ..._workspaces.map((w) =>
                                        DropdownMenuItem(
                                            value: w.id,
                                            child: Text(w.name))),
                                  ],
                                  onChanged: (v) =>
                                      setState(() => _workspaceId = v),
                                ),
                              const SizedBox(height: 16),
                              Row(
                                children: [
                                  Expanded(
                                    child: SkillSelector(
                                      availableSkills: _skills,
                                      selectedIds: _selectedSkills,
                                      onChanged: (ids) =>
                                          setState(() => _selectedSkills = ids),
                                    ),
                                  ),
                                  const SizedBox(width: 16),
                                  Expanded(
                                    child: ToolSelector(
                                      availableTools: _tools,
                                      selectedIds: _selectedTools,
                                      onChanged: (ids) =>
                                          setState(() => _selectedTools = ids),
                                    ),
                                  ),
                                ],
                              ),
                              const SizedBox(height: 16),
                              ExpansionTile(
                                title: const Text('Advanced Options'),
                                tilePadding: EdgeInsets.zero,
                                children: [
                                  DropdownButtonFormField<String?>(
                                    value: _model,
                                    decoration: const InputDecoration(
                                        labelText: 'Model Override'),
                                    items: const [
                                      DropdownMenuItem(
                                          value: null,
                                          child: Text('Default')),
                                      DropdownMenuItem(
                                          value: 'sonnet',
                                          child: Text('Sonnet')),
                                      DropdownMenuItem(
                                          value: 'opus',
                                          child: Text('Opus')),
                                      DropdownMenuItem(
                                          value: 'haiku',
                                          child: Text('Haiku')),
                                    ],
                                    onChanged: (v) =>
                                        setState(() => _model = v),
                                  ),
                                  const SizedBox(height: 12),
                                  Row(
                                    children: [
                                      Expanded(
                                        child: Column(
                                          crossAxisAlignment:
                                              CrossAxisAlignment.start,
                                          children: [
                                            Text(
                                              'Priority: ${_priority.round()}',
                                              style: const TextStyle(
                                                  fontSize: 13),
                                            ),
                                            Slider(
                                              value: _priority,
                                              min: 0,
                                              max: 9,
                                              divisions: 9,
                                              label: _priority
                                                  .round()
                                                  .toString(),
                                              onChanged: (v) => setState(
                                                  () => _priority = v),
                                            ),
                                          ],
                                        ),
                                      ),
                                      const SizedBox(width: 16),
                                      Expanded(
                                        child: TextField(
                                          controller: _maxBudgetCtrl,
                                          decoration: const InputDecoration(
                                              labelText: 'Max Budget (USD)'),
                                          keyboardType: TextInputType.number,
                                        ),
                                      ),
                                    ],
                                  ),
                                  const SizedBox(height: 12),
                                  TextField(
                                    controller: _workingDirCtrl,
                                    decoration: const InputDecoration(
                                        labelText: 'Working Directory'),
                                  ),
                                  const SizedBox(height: 12),
                                  TextField(
                                    controller: _tagsCtrl,
                                    decoration: const InputDecoration(
                                        labelText: 'Tags (comma-separated)'),
                                  ),
                                  const SizedBox(height: 12),
                                  const Text(
                                    'Output Destination',
                                    style: TextStyle(fontSize: 13),
                                  ),
                                  const SizedBox(height: 4),
                                  SegmentedButton<String>(
                                    segments: const [
                                      ButtonSegment(
                                          value: 'redis',
                                          label: Text('Redis')),
                                      ButtonSegment(
                                          value: 'file',
                                          label: Text('File')),
                                      ButtonSegment(
                                          value: 'webhook',
                                          label: Text('Webhook')),
                                    ],
                                    selected: {_outputType},
                                    onSelectionChanged: (v) => setState(
                                        () => _outputType = v.first),
                                  ),
                                  if (_outputType == 'file') ...[
                                    const SizedBox(height: 8),
                                    TextField(
                                      controller: _outputPathCtrl,
                                      decoration: const InputDecoration(
                                          labelText: 'Output Path'),
                                    ),
                                  ],
                                  if (_outputType == 'webhook') ...[
                                    const SizedBox(height: 8),
                                    TextField(
                                      controller: _webhookUrlCtrl,
                                      decoration: const InputDecoration(
                                          labelText: 'Webhook URL'),
                                    ),
                                  ],
                                  const SizedBox(height: 8),
                                ],
                              ),
                              if (_error != null) ...[
                                const SizedBox(height: 8),
                                Semantics(
                                  label: 'Error: $_error',
                                  child: Text(
                                    _error!,
                                    style:
                                        const TextStyle(color: Colors.red),
                                  ),
                                ),
                              ],
                              const SizedBox(height: 32),
                            ],
                          ),
                        ),
                      ),
                    ),
            ),
          ],
        ),
      ),
    );
  }
}
