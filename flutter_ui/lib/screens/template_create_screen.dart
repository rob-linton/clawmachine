import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/skill.dart';
import '../models/tool.dart';
import '../models/workspace.dart';
import '../widgets/skill_selector.dart';
import '../widgets/tool_selector.dart';

class TemplateCreateScreen extends ConsumerStatefulWidget {
  final String? templateId;
  const TemplateCreateScreen({super.key, this.templateId});

  @override
  ConsumerState<TemplateCreateScreen> createState() =>
      _TemplateCreateScreenState();
}

class _TemplateCreateScreenState extends ConsumerState<TemplateCreateScreen> {
  bool _loading = true;
  bool _saving = false;
  String? _error;

  final _nameCtrl = TextEditingController();
  final _descCtrl = TextEditingController();
  final _promptCtrl = TextEditingController();
  final _timeoutCtrl = TextEditingController();
  final _allowedToolsCtrl = TextEditingController();
  final _tagsCtrl = TextEditingController();
  final _outputPathCtrl = TextEditingController();
  final _webhookUrlCtrl = TextEditingController();

  String? _model;
  String? _workspaceId;
  double _priority = 5;
  Set<String> _selectedSkills = {};
  Set<String> _selectedTools = {};
  String _outputType = 'redis';

  List<Skill> _skills = [];
  List<Tool> _tools = [];
  List<Workspace> _workspaces = [];

  @override
  void initState() {
    super.initState();
    _loadData();
  }

  @override
  void dispose() {
    _nameCtrl.dispose();
    _descCtrl.dispose();
    _promptCtrl.dispose();
    _timeoutCtrl.dispose();
    _allowedToolsCtrl.dispose();
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
      ]);
      _skills = results[0] as List<Skill>;
      _tools = results[1] as List<Tool>;
      _workspaces = results[2] as List<Workspace>;

      if (widget.templateId != null) {
        final t = await api.getJobTemplate(widget.templateId!);
        _nameCtrl.text = t['name'] ?? '';
        _descCtrl.text = t['description'] ?? '';
        _promptCtrl.text = t['prompt'] ?? '';
        _timeoutCtrl.text = (t['timeout_secs'] ?? 1800).toString();
        _allowedToolsCtrl.text =
            (t['allowed_tools'] as List?)?.join(', ') ?? '';
        _tagsCtrl.text = (t['tags'] as List?)?.join(', ') ?? '';
        _model = t['model'];
        _workspaceId = t['workspace_id'];
        _priority = (t['priority'] ?? 5).toDouble();
        _selectedSkills = Set<String>.from(t['skill_ids'] ?? []);
        _selectedTools = Set<String>.from(t['tool_ids'] ?? []);
        if (t['output_dest'] is Map) {
          final od = t['output_dest'];
          if (od['type'] == 'file') {
            _outputType = 'file';
            _outputPathCtrl.text = od['path'] ?? '';
          }
          if (od['type'] == 'webhook') {
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
    if (_nameCtrl.text.trim().isEmpty || _promptCtrl.text.trim().isEmpty) {
      setState(() => _error = 'Name and prompt are required');
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
    final allowedTools = _allowedToolsCtrl.text
        .split(',')
        .map((t) => t.trim())
        .where((t) => t.isNotEmpty)
        .toList();

    final data = <String, dynamic>{
      'name': _nameCtrl.text.trim(),
      'description': _descCtrl.text.trim(),
      'prompt': _promptCtrl.text.trim(),
      if (_model != null) 'model': _model,
      if (_workspaceId != null) 'workspace_id': _workspaceId,
      if (_selectedSkills.isNotEmpty) 'skill_ids': _selectedSkills.toList(),
      if (_selectedTools.isNotEmpty) 'tool_ids': _selectedTools.toList(),
      'priority': _priority.round(),
      if (int.tryParse(_timeoutCtrl.text.trim()) != null)
        'timeout_secs': int.parse(_timeoutCtrl.text.trim()),
      if (allowedTools.isNotEmpty) 'allowed_tools': allowedTools,
      if (tags.isNotEmpty) 'tags': tags,
      if (outputDest != null) 'output_dest': outputDest,
    };

    try {
      final api = ref.read(apiClientProvider);
      if (widget.templateId != null) {
        await api.updateJobTemplate(widget.templateId!, data);
      } else {
        await api.createJobTemplate(data);
      }
      if (mounted) context.go('/templates');
    } catch (e) {
      setState(() {
        _error = 'Failed: $e';
        _saving = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final isEdit = widget.templateId != null;
    final title = isEdit ? 'Edit Template' : 'Create Template';

    return Scaffold(
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          children: [
            Row(
              children: [
                IconButton(
                  onPressed: () => context.go('/templates'),
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
                                decoration: const InputDecoration(
                                    labelText: 'Name'),
                              ),
                              const SizedBox(height: 16),
                              TextField(
                                controller: _descCtrl,
                                decoration: const InputDecoration(
                                    labelText: 'Description'),
                              ),
                              const SizedBox(height: 16),
                              TextField(
                                controller: _promptCtrl,
                                decoration: const InputDecoration(
                                  labelText: 'Prompt',
                                  alignLabelWithHint: true,
                                ),
                                minLines: 8,
                                maxLines: null,
                                style: const TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 13,
                                ),
                              ),
                              const SizedBox(height: 16),
                              Row(
                                children: [
                                  Expanded(
                                    child: DropdownButtonFormField<String?>(
                                      value: _model,
                                      decoration: const InputDecoration(
                                          labelText: 'Model'),
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
                                  ),
                                  const SizedBox(width: 16),
                                  Expanded(
                                    child: DropdownButtonFormField<String?>(
                                      value: _workspaceId,
                                      decoration: const InputDecoration(
                                          labelText: 'Workspace'),
                                      items: [
                                        const DropdownMenuItem(
                                            value: null,
                                            child: Text('None')),
                                        ..._workspaces.map((w) =>
                                            DropdownMenuItem(
                                                value: w.id,
                                                child: Text(w.name))),
                                      ],
                                      onChanged: (v) =>
                                          setState(() => _workspaceId = v),
                                    ),
                                  ),
                                ],
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
                                              label:
                                                  _priority.round().toString(),
                                              onChanged: (v) =>
                                                  setState(() => _priority = v),
                                            ),
                                          ],
                                        ),
                                      ),
                                      const SizedBox(width: 16),
                                      Expanded(
                                        child: TextField(
                                          controller: _timeoutCtrl,
                                          decoration: const InputDecoration(
                                              labelText: 'Timeout (seconds)'),
                                          keyboardType: TextInputType.number,
                                        ),
                                      ),
                                    ],
                                  ),
                                  const SizedBox(height: 12),
                                  TextField(
                                    controller: _allowedToolsCtrl,
                                    decoration: const InputDecoration(
                                      labelText: 'Allowed Tools',
                                      hintText:
                                          'Read,Write,Edit,Glob,Grep,Bash',
                                    ),
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
                                          value: 'file', label: Text('File')),
                                      ButtonSegment(
                                          value: 'webhook',
                                          label: Text('Webhook')),
                                    ],
                                    selected: {_outputType},
                                    onSelectionChanged: (v) =>
                                        setState(() => _outputType = v.first),
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
                                    style: const TextStyle(color: Colors.red),
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
