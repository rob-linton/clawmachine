import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/job.dart';
import '../models/skill.dart';
import '../models/workspace.dart';

class SubmitJobScreen extends ConsumerStatefulWidget {
  const SubmitJobScreen({super.key});

  @override
  ConsumerState<SubmitJobScreen> createState() => _SubmitJobScreenState();
}

class _SubmitJobScreenState extends ConsumerState<SubmitJobScreen> {
  final _promptController = TextEditingController();
  final _workingDirController = TextEditingController();
  final _timeoutController = TextEditingController(text: '1800');
  final _outputPathController = TextEditingController();
  final _webhookUrlController = TextEditingController();
  final _allowedToolsController = TextEditingController();
  String? _model;
  double _priority = 5;
  final _selectedSkills = <String>{};
  List<Skill> _availableSkills = [];
  List<Workspace> _availableWorkspaces = [];
  List<dynamic> _availableTemplates = [];
  List<Job> _recentJobs = [];
  List<String> _workspaceSkillNames = [];
  final _selectedPreviousJobs = <String>{};
  String? _selectedWorkspaceId;
  bool _submitting = false;
  bool _showAdvanced = false;
  String _outputType = 'redis';

  @override
  void initState() {
    super.initState();
    _loadData();
    // Read prefill params from URL (from "Use in New Job")
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final uri = GoRouterState.of(context).uri;
      final prefillResult = uri.queryParameters['prefill_result'];
      final wsId = uri.queryParameters['workspace_id'];
      final model = uri.queryParameters['model'];
      if (prefillResult != null && prefillResult.isNotEmpty) {
        _promptController.text = '<previous_result>\n$prefillResult\n</previous_result>\n\n';
      }
      if (wsId != null && wsId.isNotEmpty) {
        setState(() => _selectedWorkspaceId = wsId);
      }
      if (model != null && model.isNotEmpty) {
        setState(() => _model = model);
      }
    });
  }

  Future<void> _loadData() async {
    final api = ref.read(apiClientProvider);
    try {
      final skills = await api.listSkills();
      setState(() => _availableSkills = skills);
    } catch (_) {}
    try {
      final workspaces = await api.listWorkspaces();
      setState(() => _availableWorkspaces = workspaces);
    } catch (_) {}
    try {
      final jobs = await api.listJobs(status: 'completed', limit: 10);
      setState(() => _recentJobs = jobs);
    } catch (_) {}
    try {
      final templates = await api.listJobTemplates();
      setState(() => _availableTemplates = templates);
    } catch (_) {}
  }

  void _applyTemplate(dynamic template) {
    if (template == null) return;
    setState(() {
      _promptController.text = template['prompt'] ?? '';
      _model = template['model'];
      _priority = (template['priority'] ?? 5).toDouble();
      _selectedWorkspaceId = template['workspace_id'];
      final skillIds = List<String>.from(template['skill_ids'] ?? []);
      _selectedSkills.clear();
      _selectedSkills.addAll(skillIds);
      if (template['timeout_secs'] != null) {
        _timeoutController.text = template['timeout_secs'].toString();
      }
      if (_selectedWorkspaceId != null) {
        _loadWorkspaceSkills(_selectedWorkspaceId);
      }
    });
  }

  Map<String, dynamic>? _buildOutputDest() {
    switch (_outputType) {
      case 'file':
        final path = _outputPathController.text.trim();
        if (path.isEmpty) return null;
        return {'File': {'path': path}};
      case 'webhook':
        final url = _webhookUrlController.text.trim();
        if (url.isEmpty) return null;
        return {'Webhook': {'url': url}};
      default:
        return null; // Redis is default
    }
  }

  Future<void> _loadWorkspaceSkills(String? wsId) async {
    if (wsId == null) {
      setState(() => _workspaceSkillNames = []);
      return;
    }
    try {
      final files = await ref.read(apiClientProvider).listWorkspaceFiles(wsId);
      final skillNames = <String>[];
      for (final f in files) {
        final path = (f['path'] ?? '').toString();
        // Match .claude/skills/*/SKILL.md
        if (path.startsWith('.claude/skills/') && path.endsWith('/SKILL.md')) {
          final parts = path.split('/');
          if (parts.length >= 4) {
            skillNames.add(parts[2]); // the skill directory name
          }
        }
      }
      setState(() => _workspaceSkillNames = skillNames);
    } catch (_) {
      setState(() => _workspaceSkillNames = []);
    }
  }

  int _estimatePromptSize() {
    var size = _promptController.text.length;
    // Add context from selected previous results
    for (final jobId in _selectedPreviousJobs) {
      final job = _recentJobs.where((j) => j.id == jobId).firstOrNull;
      if (job != null) {
        size += job.prompt.length.clamp(0, 200) + 100; // approximate context wrapper
      }
    }
    // Add metadata overhead
    size += 100; // [Job ID: ...] [Source: ...] etc.
    return size;
  }

  String _buildPromptWithContext() {
    var prompt = _promptController.text.trim();
    // Inject selected previous results
    if (_selectedPreviousJobs.isNotEmpty) {
      final context = StringBuffer();
      for (final jobId in _selectedPreviousJobs) {
        final job = _recentJobs.where((j) => j.id == jobId).firstOrNull;
        if (job != null) {
          context.writeln('<previous_result job_id="${job.shortId}">');
          final preview = job.prompt.length > 100 ? '${job.prompt.substring(0, 100)}...' : job.prompt;
          context.writeln('Prompt: $preview');
          context.writeln('(Result will be fetched at execution time)');
          context.writeln('</previous_result>');
          context.writeln();
        }
      }
      prompt = '${context}$prompt';
    }
    return prompt;
  }

  Future<void> _submit() async {
    final prompt = _buildPromptWithContext();
    if (prompt.isEmpty) return;

    setState(() => _submitting = true);
    try {
      final workingDir = _workingDirController.text.trim();
      final timeout = int.tryParse(_timeoutController.text.trim());
      final allowedTools = _allowedToolsController.text.trim();

      final resp = await ref.read(apiClientProvider).submitJob(
            prompt: prompt,
            skillIds: _selectedSkills.toList(),
            model: _model,
            priority: _priority.round(),
            workspaceId: _selectedWorkspaceId,
            workingDir: workingDir.isEmpty || _selectedWorkspaceId != null ? null : workingDir,
            timeoutSecs: timeout,
            outputDest: _buildOutputDest(),
            allowedTools: allowedTools.isEmpty
                ? null
                : allowedTools.split(',').map((t) => t.trim()).toList(),
          );
      if (mounted) {
        context.go('/jobs/${resp['id']}');
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Submit failed: $e')));
      }
    } finally {
      setState(() => _submitting = false);
    }
  }

  @override
  void dispose() {
    _promptController.dispose();
    _workingDirController.dispose();
    _timeoutController.dispose();
    _outputPathController.dispose();
    _webhookUrlController.dispose();
    _allowedToolsController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Submit New Job',
              style: Theme.of(context).textTheme.headlineMedium),
          const SizedBox(height: 24),
          Expanded(
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Template selector
                  if (_availableTemplates.isNotEmpty) ...[
                    DropdownButtonFormField<String?>(
                      value: null,
                      decoration: const InputDecoration(
                        labelText: 'Use Template (optional)',
                        border: OutlineInputBorder(),
                        helperText: 'Select a template to pre-fill the form',
                      ),
                      items: [
                        const DropdownMenuItem(value: null, child: Text('None (ad-hoc)')),
                        ..._availableTemplates.map((t) => DropdownMenuItem(
                              value: t['id'] as String?,
                              child: Text(t['name'] ?? ''),
                            )),
                      ],
                      onChanged: (id) {
                        if (id != null) {
                          final tmpl = _availableTemplates.firstWhere((t) => t['id'] == id, orElse: () => null);
                          _applyTemplate(tmpl);
                        }
                      },
                    ),
                    const SizedBox(height: 24),
                  ],

                  // Prompt
                  Text('Prompt',
                      style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  TextField(
                    controller: _promptController,
                    maxLines: 6,
                    style: const TextStyle(fontFamily: 'monospace'),
                    decoration: const InputDecoration(
                      hintText: 'Enter your task prompt...',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 24),

                  // Workspace Skills (read-only info)
                  if (_workspaceSkillNames.isNotEmpty) ...[
                    Text('Workspace Skills (already in workspace)',
                        style: Theme.of(context).textTheme.titleSmall),
                    const SizedBox(height: 4),
                    Wrap(
                      spacing: 6,
                      children: _workspaceSkillNames
                          .map((name) => Chip(
                                label: Text(name, style: const TextStyle(fontSize: 12)),
                                backgroundColor: Colors.green.withValues(alpha: 0.15),
                                visualDensity: VisualDensity.compact,
                              ))
                          .toList(),
                    ),
                    const SizedBox(height: 16),
                  ],

                  // Additional Skills (injected at job time)
                  if (_availableSkills.isNotEmpty) ...[
                    Text(_workspaceSkillNames.isNotEmpty
                            ? 'Additional Skills (injected for this job)'
                            : 'Skills',
                        style: Theme.of(context).textTheme.titleMedium),
                    const SizedBox(height: 8),
                    Wrap(
                      spacing: 8,
                      children: _availableSkills.map((s) {
                        final selected = _selectedSkills.contains(s.id);
                        return FilterChip(
                          label: Text(s.name),
                          selected: selected,
                          onSelected: (v) {
                            setState(() {
                              if (v) {
                                _selectedSkills.add(s.id);
                              } else {
                                _selectedSkills.remove(s.id);
                              }
                            });
                          },
                        );
                      }).toList(),
                    ),
                    const SizedBox(height: 24),
                  ],
                  if (_availableSkills.isEmpty && _workspaceSkillNames.isEmpty)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 16),
                      child: Text('No skills imported yet. Go to Skills to import.',
                          style: TextStyle(color: Colors.grey[500], fontSize: 13)),
                    ),

                  // Model + Priority row
                  Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text('Model',
                                style:
                                    Theme.of(context).textTheme.titleMedium),
                            const SizedBox(height: 8),
                            DropdownButtonFormField<String>(
                              initialValue: _model,
                              decoration: const InputDecoration(
                                  border: OutlineInputBorder(),
                                  hintText: 'Default'),
                              items: const [
                                DropdownMenuItem(
                                    value: null, child: Text('Default')),
                                DropdownMenuItem(
                                    value: 'sonnet', child: Text('Sonnet')),
                                DropdownMenuItem(
                                    value: 'opus', child: Text('Opus')),
                                DropdownMenuItem(
                                    value: 'haiku', child: Text('Haiku')),
                              ],
                              onChanged: (v) => setState(() => _model = v),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: 24),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                                'Priority: ${_priority.round()}',
                                style:
                                    Theme.of(context).textTheme.titleMedium),
                            Slider(
                              value: _priority,
                              min: 0,
                              max: 9,
                              divisions: 9,
                              label: _priority.round().toString(),
                              onChanged: (v) =>
                                  setState(() => _priority = v),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),

                  // Previous Results
                  if (_recentJobs.isNotEmpty) ...[
                    ExpansionTile(
                      title: const Text('Include Previous Results'),
                      subtitle: _selectedPreviousJobs.isNotEmpty
                          ? Text('${_selectedPreviousJobs.length} selected')
                          : null,
                      children: [
                        Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                          child: Column(
                            children: _recentJobs.map((job) {
                              final selected = _selectedPreviousJobs.contains(job.id);
                              return CheckboxListTile(
                                dense: true,
                                value: selected,
                                title: Text(job.promptPreview,
                                    style: const TextStyle(fontSize: 13)),
                                subtitle: Text('${job.shortId} - ${job.status}',
                                    style: const TextStyle(fontSize: 11)),
                                onChanged: (v) {
                                  setState(() {
                                    if (v == true) {
                                      _selectedPreviousJobs.add(job.id);
                                    } else {
                                      _selectedPreviousJobs.remove(job.id);
                                    }
                                  });
                                },
                              );
                            }).toList(),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 16),
                  ],

                  // Advanced Options
                  ExpansionTile(
                    title: const Text('Advanced Options'),
                    initiallyExpanded: _showAdvanced,
                    onExpansionChanged: (v) => setState(() => _showAdvanced = v),
                    children: [
                      Padding(
                        padding: const EdgeInsets.all(16),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            // Workspace selector
                            DropdownButtonFormField<String?>(
                              value: _selectedWorkspaceId,
                              decoration: const InputDecoration(
                                labelText: 'Workspace',
                                border: OutlineInputBorder(),
                              ),
                              items: [
                                const DropdownMenuItem(
                                  value: null,
                                  child: Text('None (temp workspace)'),
                                ),
                                ..._availableWorkspaces.map((ws) =>
                                    DropdownMenuItem(
                                      value: ws.id,
                                      child: Text('${ws.name} — ${ws.path}'),
                                    )),
                              ],
                              onChanged: (v) =>
                                  setState(() {
                                    _selectedWorkspaceId = v;
                                    _loadWorkspaceSkills(v);
                                  }),
                            ),
                            const SizedBox(height: 12),
                            if (_selectedWorkspaceId == null)
                              TextField(
                                controller: _workingDirController,
                                decoration: const InputDecoration(
                                  labelText: 'Working Directory (if no workspace)',
                                  hintText: '/path/to/project',
                                  border: OutlineInputBorder(),
                                ),
                              ),
                            if (_selectedWorkspaceId == null)
                            const SizedBox(height: 12),
                            TextField(
                              controller: _timeoutController,
                              decoration: const InputDecoration(
                                labelText: 'Timeout (seconds)',
                                border: OutlineInputBorder(),
                              ),
                              keyboardType: TextInputType.number,
                            ),
                            const SizedBox(height: 12),
                            TextField(
                              controller: _allowedToolsController,
                              decoration: const InputDecoration(
                                labelText: 'Allowed Tools (comma-separated)',
                                hintText: 'Read,Grep,Glob',
                                border: OutlineInputBorder(),
                              ),
                            ),
                            const SizedBox(height: 16),
                            Text('Output Destination',
                                style: Theme.of(context).textTheme.titleSmall),
                            const SizedBox(height: 8),
                            SegmentedButton<String>(
                              segments: const [
                                ButtonSegment(value: 'redis', label: Text('Redis')),
                                ButtonSegment(value: 'file', label: Text('File')),
                                ButtonSegment(value: 'webhook', label: Text('Webhook')),
                              ],
                              selected: {_outputType},
                              onSelectionChanged: (v) =>
                                  setState(() => _outputType = v.first),
                            ),
                            if (_outputType == 'file') ...[
                              const SizedBox(height: 8),
                              TextField(
                                controller: _outputPathController,
                                decoration: const InputDecoration(
                                  labelText: 'Output Directory',
                                  hintText: '/path/to/output',
                                  border: OutlineInputBorder(),
                                ),
                              ),
                            ],
                            if (_outputType == 'webhook') ...[
                              const SizedBox(height: 8),
                              TextField(
                                controller: _webhookUrlController,
                                decoration: const InputDecoration(
                                  labelText: 'Webhook URL',
                                  hintText: 'https://example.com/webhook',
                                  border: OutlineInputBorder(),
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),

                  // Prompt size estimate
                  Builder(builder: (context) {
                    final size = _estimatePromptSize();
                    final sizeKb = (size / 1024).toStringAsFixed(1);
                    final color = size > 50000
                        ? Colors.red
                        : size > 20000
                            ? Colors.orange
                            : Colors.grey;
                    return Text(
                      'Estimated prompt size: ~$sizeKb KB ($size chars)',
                      style: TextStyle(fontSize: 12, color: color),
                    );
                  }),
                  const SizedBox(height: 16),

                  // Submit button
                  SizedBox(
                    width: double.infinity,
                    height: 48,
                    child: FilledButton.icon(
                      onPressed: _submitting ? null : _submit,
                      icon: _submitting
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(
                                  strokeWidth: 2))
                          : const Icon(Icons.send),
                      label: Text(_submitting ? 'Submitting...' : 'Submit Job'),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
