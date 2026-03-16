import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
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
  String? _selectedWorkspaceId;
  bool _submitting = false;
  bool _showAdvanced = false;
  String _outputType = 'redis'; // redis, file, webhook

  @override
  void initState() {
    super.initState();
    _loadData();
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

  Future<void> _submit() async {
    final prompt = _promptController.text.trim();
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

                  // Skills
                  if (_availableSkills.isNotEmpty) ...[
                    Text('Skills',
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
                                  setState(() => _selectedWorkspaceId = v),
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
                  const SizedBox(height: 32),

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
