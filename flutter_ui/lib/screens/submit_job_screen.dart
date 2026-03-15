import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/skill.dart';

class SubmitJobScreen extends ConsumerStatefulWidget {
  const SubmitJobScreen({super.key});

  @override
  ConsumerState<SubmitJobScreen> createState() => _SubmitJobScreenState();
}

class _SubmitJobScreenState extends ConsumerState<SubmitJobScreen> {
  final _promptController = TextEditingController();
  String? _model;
  double _priority = 5;
  final _selectedSkills = <String>{};
  List<Skill> _availableSkills = [];
  bool _submitting = false;

  @override
  void initState() {
    super.initState();
    _loadSkills();
  }

  Future<void> _loadSkills() async {
    try {
      final skills = await ref.read(apiClientProvider).listSkills();
      setState(() => _availableSkills = skills);
    } catch (_) {}
  }

  Future<void> _submit() async {
    final prompt = _promptController.text.trim();
    if (prompt.isEmpty) return;

    setState(() => _submitting = true);
    try {
      final resp = await ref.read(apiClientProvider).submitJob(
            prompt: prompt,
            skillIds: _selectedSkills.toList(),
            model: _model,
            priority: _priority.round(),
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
                      // Model
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
                      // Priority
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
