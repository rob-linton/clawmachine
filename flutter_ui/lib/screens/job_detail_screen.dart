import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
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

      // Load result if completed
      if (job.status == 'completed') {
        try {
          final result = await api.getResult(widget.jobId);
          setState(() => _result = result);
        } catch (_) {}
      }

      // Load logs
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

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    final job = _job;
    if (job == null) {
      return const Center(child: Text('Job not found'));
    }

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
                  onPressed: () => Navigator.of(context).maybePop(),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text('Job ${job.shortId}',
                      style: Theme.of(context).textTheme.headlineMedium),
                ),
                StatusBadge(status: job.status),
                const SizedBox(width: 16),
                if (job.status == 'pending' || job.status == 'running')
                  OutlinedButton.icon(
                    onPressed: _cancel,
                    icon: const Icon(Icons.cancel),
                    label: const Text('Cancel'),
                  ),
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
                    _row('Created', job.createdAt),
                    if (job.startedAt != null)
                      _row('Started', job.startedAt!),
                    if (job.completedAt != null)
                      _row('Completed', job.completedAt!),
                    if (job.model != null) _row('Model', job.model!),
                    if (job.workerId != null) _row('Worker', job.workerId!),
                    if (job.costUsd != null)
                      _row('Cost', '\$${job.costUsd!.toStringAsFixed(4)}'),
                    if (job.durationMs != null)
                      _row('Duration',
                          '${(job.durationMs! / 1000).toStringAsFixed(1)}s'),
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
