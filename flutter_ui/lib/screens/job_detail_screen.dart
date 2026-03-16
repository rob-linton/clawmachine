import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/job.dart';
import '../widgets/status_badge.dart';

class JobDetailScreen extends ConsumerStatefulWidget {
  final String jobId;
  const JobDetailScreen({super.key, required this.jobId});

  @override
  ConsumerState<JobDetailScreen> createState() => _JobDetailScreenState();
}

class _JobDetailScreenState extends ConsumerState<JobDetailScreen> {
  Job? _job;
  JobResult? _result;
  List<String> _logs = [];
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
      final job = await api.getJob(widget.jobId);
      setState(() {
        _job = job;
        _loading = false;
      });

      if (job.status == 'completed') {
        try {
          final result = await api.getResult(widget.jobId);
          setState(() => _result = result);
        } catch (_) {}
      }

      try {
        final logs = await api.getLogs(widget.jobId);
        setState(() => _logs = logs);
      } catch (_) {}
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _cancel() async {
    try {
      await ref.read(apiClientProvider).cancelJob(widget.jobId);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Cancel failed: $e')));
      }
    }
  }

  Future<void> _delete() async {
    final confirm = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Job'),
        content: const Text('Delete this job and all its data?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            style: FilledButton.styleFrom(backgroundColor: Colors.red),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).deleteJob(widget.jobId);
      if (mounted) context.go('/jobs');
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
  }

  Future<void> _resubmit() async {
    final job = _job;
    if (job == null) return;
    try {
      final resp = await ref.read(apiClientProvider).submitJob(
            prompt: job.prompt,
            skillIds: job.skillIds,
            skillTags: job.skillTags,
            model: job.model,
            priority: job.priority,
            tags: job.tags,
            workingDir: job.workingDir != '.' ? job.workingDir : null,
            outputDest: job.outputDest,
            allowedTools: job.allowedTools,
          );
      if (mounted) {
        context.go('/jobs/${resp['id']}');
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Re-submit failed: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    final job = _job;
    if (job == null) {
      return const Center(child: Text('Job not found'));
    }

    final isTerminal = job.status == 'completed' ||
        job.status == 'failed' ||
        job.status == 'cancelled';

    return Padding(
      padding: const EdgeInsets.all(24),
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              children: [
                IconButton(
                  icon: const Icon(Icons.arrow_back),
                  onPressed: () => context.go('/jobs'),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text('Job ${job.shortId}',
                      style: Theme.of(context).textTheme.headlineMedium),
                ),
                StatusBadge(status: job.status),
                const SizedBox(width: 8),
                if (job.status == 'pending' || job.status == 'running')
                  OutlinedButton.icon(
                    onPressed: _cancel,
                    icon: const Icon(Icons.cancel),
                    label: const Text('Cancel'),
                  ),
                if (isTerminal) ...[
                  OutlinedButton.icon(
                    onPressed: _resubmit,
                    icon: const Icon(Icons.replay),
                    label: const Text('Re-submit'),
                  ),
                  const SizedBox(width: 8),
                  OutlinedButton.icon(
                    onPressed: _delete,
                    icon: const Icon(Icons.delete, color: Colors.red),
                    label: const Text('Delete'),
                  ),
                ],
                const SizedBox(width: 8),
                IconButton(
                    onPressed: _refresh, icon: const Icon(Icons.refresh)),
              ],
            ),
            const SizedBox(height: 24),

            // Metadata
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    _row('ID', job.id),
                    _row('Status', job.status),
                    if (job.source != null) _row('Source', job.source!),
                    _row('Created', job.createdAt),
                    if (job.startedAt != null) _row('Started', job.startedAt!),
                    if (job.completedAt != null) _row('Completed', job.completedAt!),
                    if (job.model != null) _row('Model', job.model!),
                    if (job.workerId != null) _row('Worker', job.workerId!),
                    if (job.costUsd != null)
                      _row('Cost', '\$${job.costUsd!.toStringAsFixed(4)}'),
                    if (job.durationMs != null)
                      _row('Duration',
                          '${(job.durationMs! / 1000).toStringAsFixed(1)}s'),
                    if (job.workingDir != '.')
                      _row('Working Dir', job.workingDir),
                    if (job.timeoutSecs != null)
                      _row('Timeout', '${job.timeoutSecs}s'),
                    if (job.retryCount > 0)
                      _row('Retries', job.retryCount.toString()),
                    if (job.cronId != null) _row('Cron ID', job.cronId!),
                    _row('Priority', job.priority.toString()),
                    if (job.tags.isNotEmpty) _row('Tags', job.tags.join(', ')),
                    if (job.skillIds.isNotEmpty)
                      _row('Skills', job.skillIds.join(', ')),
                    if (job.error != null) _row('Error', job.error!),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 16),

            // Prompt
            Text('Prompt', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: SelectableText(job.prompt,
                    style: const TextStyle(fontFamily: 'monospace')),
              ),
            ),
            const SizedBox(height: 16),

            // Result
            if (_result != null) ...[
              Text('Result', style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              Card(
                color: Theme.of(context).colorScheme.primaryContainer,
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: SelectableText(_result!.result,
                      style: const TextStyle(fontFamily: 'monospace')),
                ),
              ),
              const SizedBox(height: 16),
            ],

            // Skill Snapshot
            if (job.skillSnapshot != null) ...[
              ExpansionTile(
                title: const Text('Skill Snapshot'),
                children: [
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: SelectableText(
                      const JsonEncoder.withIndent('  ')
                          .convert(job.skillSnapshot),
                      style: const TextStyle(
                          fontFamily: 'monospace', fontSize: 12),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
            ],

            // Assembled Prompt
            if (job.assembledPrompt != null) ...[
              ExpansionTile(
                title: const Text('Assembled Prompt'),
                children: [
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: SelectableText(
                      job.assembledPrompt!,
                      style: const TextStyle(
                          fontFamily: 'monospace', fontSize: 12),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
            ],

            // Logs
            if (_logs.isNotEmpty) ...[
              Text('Logs (${_logs.length} lines)',
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              Card(
                color: Colors.black87,
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: SizedBox(
                    height: 300,
                    child: ListView.builder(
                      itemCount: _logs.length,
                      itemBuilder: (context, i) => Text(
                        _formatLogLine(_logs[i]),
                        style: const TextStyle(
                          fontFamily: 'monospace',
                          fontSize: 12,
                          color: Colors.greenAccent,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _row(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
              width: 100,
              child: Text('$label:',
                  style: const TextStyle(fontWeight: FontWeight.bold))),
          Expanded(child: SelectableText(value)),
        ],
      ),
    );
  }

  String _formatLogLine(String raw) {
    return raw.length > 120 ? '${raw.substring(0, 120)}...' : raw;
  }
}
