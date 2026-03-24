import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/workspace.dart';

class _StepState {
  final TextEditingController nameCtrl;
  final TextEditingController promptCtrl;
  String? templateId;
  String? model;

  _StepState()
      : nameCtrl = TextEditingController(),
        promptCtrl = TextEditingController();

  void dispose() {
    nameCtrl.dispose();
    promptCtrl.dispose();
  }
}

class PipelineCreateScreen extends ConsumerStatefulWidget {
  final String? pipelineId;
  const PipelineCreateScreen({super.key, this.pipelineId});

  @override
  ConsumerState<PipelineCreateScreen> createState() =>
      _PipelineCreateScreenState();
}

class _PipelineCreateScreenState extends ConsumerState<PipelineCreateScreen> {
  bool _loading = true;
  bool _saving = false;
  String? _error;
  final TextEditingController _nameCtrl = TextEditingController();
  final TextEditingController _descCtrl = TextEditingController();
  String? _workspaceId;
  List<Workspace> _workspaces = [];
  List<dynamic> _templates = [];
  final List<_StepState> _steps = [];

  bool get isEdit => widget.pipelineId != null;

  @override
  void initState() {
    super.initState();
    _loadData();
  }

  @override
  void dispose() {
    _nameCtrl.dispose();
    _descCtrl.dispose();
    for (final step in _steps) {
      step.dispose();
    }
    super.dispose();
  }

  Future<void> _loadData() async {
    try {
      final api = ref.read(apiClientProvider);
      final workspaces = await api.listWorkspaces();
      final templates = await api.listJobTemplates();

      _workspaces = workspaces;
      _templates = templates;

      if (isEdit) {
        final pipeline = await api.getPipeline(widget.pipelineId!);
        _nameCtrl.text = pipeline['name'] ?? '';
        _descCtrl.text = pipeline['description'] ?? '';
        _workspaceId = pipeline['workspace_id'];

        final steps = pipeline['steps'] as List? ?? [];
        for (final s in steps) {
          final step = _StepState();
          step.nameCtrl.text = s['name'] ?? '';
          step.templateId = s['template_id'];
          step.promptCtrl.text = s['prompt'] ?? '';
          step.model = s['model'];
          _steps.add(step);
        }
      } else {
        _steps.add(_StepState());
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
    if (_nameCtrl.text.trim().isEmpty) {
      setState(() => _error = 'Pipeline name is required');
      return;
    }
    if (_steps
        .any((s) => s.promptCtrl.text.trim().isEmpty && s.templateId == null)) {
      setState(() => _error = 'Each step needs a template or a prompt');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });

    final data = <String, dynamic>{
      'name': _nameCtrl.text.trim(),
      'description': _descCtrl.text.trim(),
      if (_workspaceId != null) 'workspace_id': _workspaceId,
      'steps': _steps.asMap().entries.map((entry) {
        return <String, dynamic>{
          'name': entry.value.nameCtrl.text.trim().isEmpty
              ? 'Step ${entry.key + 1}'
              : entry.value.nameCtrl.text.trim(),
          'prompt': entry.value.promptCtrl.text.trim(),
          if (entry.value.templateId != null)
            'template_id': entry.value.templateId,
          if (entry.value.model != null) 'model': entry.value.model,
        };
      }).toList(),
    };

    try {
      final api = ref.read(apiClientProvider);
      if (widget.pipelineId != null) {
        await api.updatePipeline(widget.pipelineId!, data);
      } else {
        await api.createPipeline(data);
      }
      if (mounted) context.go('/pipelines');
    } catch (e) {
      setState(() {
        _error = 'Failed: $e';
        _saving = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final title = isEdit ? 'Edit Pipeline' : 'New Pipeline';

    return Scaffold(
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          children: [
            Row(
              children: [
                IconButton(
                  onPressed: () => context.go('/pipelines'),
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
                                  labelText: 'Pipeline Name',
                                ),
                              ),
                              const SizedBox(height: 16),
                              TextField(
                                controller: _descCtrl,
                                decoration: const InputDecoration(
                                  labelText: 'Description',
                                ),
                              ),
                              const SizedBox(height: 16),
                              if (_workspaces.isNotEmpty)
                                DropdownButtonFormField<String?>(
                                  value: _workspaceId,
                                  decoration: const InputDecoration(
                                    labelText: 'Workspace',
                                  ),
                                  items: [
                                    const DropdownMenuItem(
                                      value: null,
                                      child: Text('None'),
                                    ),
                                    ..._workspaces.map(
                                      (w) => DropdownMenuItem(
                                        value: w.id,
                                        child: Text(w.name),
                                      ),
                                    ),
                                  ],
                                  onChanged: (v) =>
                                      setState(() => _workspaceId = v),
                                ),
                              const SizedBox(height: 24),
                              Row(
                                children: [
                                  Semantics(
                                    header: true,
                                    label: 'Steps',
                                    child: Text(
                                      'Steps',
                                      style: Theme.of(context)
                                          .textTheme
                                          .titleLarge,
                                    ),
                                  ),
                                  const Spacer(),
                                  OutlinedButton.icon(
                                    onPressed: () =>
                                        setState(() => _steps.add(_StepState())),
                                    icon: const Icon(Icons.add),
                                    label: const Text('Add Step'),
                                  ),
                                ],
                              ),
                              const SizedBox(height: 12),
                              ..._steps.asMap().entries.map((entry) {
                                final i = entry.key;
                                final step = entry.value;
                                return _buildStepCard(i, step);
                              }),
                              if (_templates.isEmpty)
                                Padding(
                                  padding: const EdgeInsets.only(top: 8),
                                  child: Row(
                                    children: [
                                      Semantics(
                                        label: 'No templates yet',
                                        child: const Text(
                                          'No templates yet. ',
                                          style: TextStyle(color: Colors.grey),
                                        ),
                                      ),
                                      TextButton(
                                        onPressed: () =>
                                            context.go('/templates/create'),
                                        child: const Text('Create a template'),
                                      ),
                                    ],
                                  ),
                                ),
                              if (_error != null) ...[
                                const SizedBox(height: 8),
                                Semantics(
                                  label: _error,
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

  Widget _buildStepCard(int i, _StepState step) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  CircleAvatar(
                    radius: 14,
                    child: Text(
                      '${i + 1}',
                      style: const TextStyle(fontSize: 12),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: TextField(
                      controller: step.nameCtrl,
                      decoration: InputDecoration(
                        labelText: 'Step Name',
                        hintText: 'Step ${i + 1}',
                        isDense: true,
                      ),
                    ),
                  ),
                  if (_steps.length > 1)
                    IconButton(
                      icon: const Icon(Icons.delete, size: 18),
                      onPressed: () => setState(() {
                        _steps[i].dispose();
                        _steps.removeAt(i);
                      }),
                    ),
                ],
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    flex: 2,
                    child: DropdownButtonFormField<String?>(
                      value: step.templateId,
                      decoration: const InputDecoration(
                        labelText: 'Template',
                        isDense: true,
                      ),
                      items: [
                        const DropdownMenuItem(
                          value: null,
                          child: Text('None (inline prompt)'),
                        ),
                        ..._templates.map(
                          (t) => DropdownMenuItem(
                            value: t['id'] as String?,
                            child: Text(t['name'] ?? ''),
                          ),
                        ),
                      ],
                      onChanged: (id) {
                        setState(() {
                          step.templateId = id;
                          if (id != null) {
                            final tmpl = _templates.firstWhere(
                              (t) => t['id'] == id,
                              orElse: () => null,
                            );
                            if (tmpl != null) {
                              if (step.nameCtrl.text.isEmpty) {
                                step.nameCtrl.text = tmpl['name'] ?? '';
                              }
                              if (step.promptCtrl.text.isEmpty) {
                                step.promptCtrl.text = tmpl['prompt'] ?? '';
                              }
                              step.model ??= tmpl['model'];
                            }
                          }
                        });
                      },
                    ),
                  ),
                  const SizedBox(width: 16),
                  Expanded(
                    child: DropdownButtonFormField<String?>(
                      value: step.model,
                      decoration: const InputDecoration(
                        labelText: 'Model',
                        isDense: true,
                      ),
                      items: const [
                        DropdownMenuItem(
                          value: null,
                          child: Text('Default'),
                        ),
                        DropdownMenuItem(
                          value: 'sonnet',
                          child: Text('Sonnet'),
                        ),
                        DropdownMenuItem(
                          value: 'opus',
                          child: Text('Opus'),
                        ),
                        DropdownMenuItem(
                          value: 'haiku',
                          child: Text('Haiku'),
                        ),
                      ],
                      onChanged: (v) => setState(() => step.model = v),
                    ),
                  ),
                ],
              ),
              if (step.templateId != null) ...[
                const SizedBox(height: 8),
                Builder(builder: (_) {
                  final tmpl = _templates.firstWhere(
                    (t) => t['id'] == step.templateId,
                    orElse: () => null,
                  );
                  if (tmpl == null) return const SizedBox.shrink();
                  final desc = tmpl['description'] ?? '';
                  return Semantics(
                    label: 'Template description: $desc',
                    child: Text(
                      desc,
                      style:
                          const TextStyle(fontSize: 12, color: Colors.grey),
                    ),
                  );
                }),
              ],
              const SizedBox(height: 12),
              TextField(
                controller: step.promptCtrl,
                decoration: InputDecoration(
                  labelText: step.templateId != null
                      ? 'Prompt Override (optional)'
                      : 'Prompt',
                  alignLabelWithHint: true,
                  helperText: i > 0
                      ? 'Use {{previous_result}} to inject previous step output'
                      : null,
                  isDense: true,
                ),
                minLines: 6,
                maxLines: null,
                style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
