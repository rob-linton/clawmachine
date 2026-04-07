import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/job.dart';
import '../widgets/status_badge.dart';
import '../widgets/tool_activity.dart';

class JobDetailScreen extends ConsumerStatefulWidget {
  final String jobId;
  const JobDetailScreen({super.key, required this.jobId});

  @override
  ConsumerState<JobDetailScreen> createState() => _JobDetailScreenState();
}

class _JobDetailScreenState extends ConsumerState<JobDetailScreen> {
  Job? _job;
  JobResult? _result;
  List<String> _rawLogs = [];
  bool _loading = true;
  StreamSubscription? _eventSub;
  Timer? _logPollTimer;
  String? _workspaceName;
  final ScrollController _logScrollController = ScrollController();

  @override
  void initState() {
    super.initState();
    _refresh();
    _eventSub = ref.read(eventServiceProvider).jobUpdates.listen((event) {
      if (event['job_id'] == widget.jobId) {
        _refresh();
      }
    });
  }

  @override
  void dispose() {
    _eventSub?.cancel();
    _logPollTimer?.cancel();
    _logScrollController.dispose();
    super.dispose();
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

      // Fetch result for completed and failed jobs
      if (job.status == 'completed' || job.status == 'failed') {
        _logPollTimer?.cancel();
        try {
          final result = await api.getResult(widget.jobId);
          setState(() => _result = result);
        } catch (_) {}
      }

      // Fetch workspace name
      if (job.workspaceId != null && _workspaceName == null) {
        try {
          final ws = await api.getWorkspace(job.workspaceId!);
          setState(() => _workspaceName = ws.name);
        } catch (_) {}
      }

      // Fetch logs
      await _fetchLogs();

      // Start polling logs while running
      if (job.status == 'running' || job.status == 'pending') {
        _logPollTimer?.cancel();
        _logPollTimer = Timer.periodic(const Duration(seconds: 3), (_) {
          _fetchLogs();
          _refreshJob();
        });
      }
    } catch (e) {
      setState(() => _loading = false);
    }
  }

  Future<void> _refreshJob() async {
    try {
      final job = await ref.read(apiClientProvider).getJob(widget.jobId);
      setState(() => _job = job);
      if (job.status == 'completed' || job.status == 'failed') {
        _logPollTimer?.cancel();
        try {
          final result =
              await ref.read(apiClientProvider).getResult(widget.jobId);
          setState(() => _result = result);
        } catch (_) {}
      }
    } catch (_) {}
  }

  Future<void> _fetchLogs() async {
    try {
      final logs = await ref.read(apiClientProvider).getLogs(widget.jobId);
      if (logs.length > _rawLogs.length) {
        setState(() => _rawLogs = logs);
        // Auto-scroll to bottom
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_logScrollController.hasClients) {
            _logScrollController.animateTo(
              _logScrollController.position.maxScrollExtent,
              duration: const Duration(milliseconds: 200),
              curve: Curves.easeOut,
            );
          }
        });
      }
    } catch (_) {}
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
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Job'),
        content: const Text('Delete this job and all its data?'),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Cancel')),
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
            maxBudget: job.maxBudgetUsd,
            timeoutSecs: job.timeoutSecs,
            workspaceId: job.workspaceId,
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
                    if (job.completedAt != null)
                      _row('Completed', job.completedAt!),
                    if (job.model != null) _row('Model', job.model!),
                    if (job.workerId != null) _row('Worker', job.workerId!),
                    if (job.costUsd != null)
                      _row('Cost', '\$${job.costUsd!.toStringAsFixed(4)}'),
                    if (job.durationMs != null)
                      _row('Duration',
                          '${(job.durationMs! / 1000).toStringAsFixed(1)}s'),
                    if (job.timeoutSecs != null)
                      _row('Timeout', _formatTimeout(job.timeoutSecs!)),
                    _row('Priority', job.priority.toString()),
                    if (job.maxBudgetUsd != null)
                      _row('Budget',
                          '\$${job.maxBudgetUsd!.toStringAsFixed(2)}'),
                    if (job.workspaceId != null)
                      _workspaceRow(job.workspaceId!),
                    if (job.templateId != null)
                      _row('Template', job.templateId!),
                    if (job.cronId != null) _row('Cron', job.cronId!),
                    if (job.allowedTools != null &&
                        job.allowedTools!.isNotEmpty)
                      _row('Tools', job.allowedTools!.join(', ')),
                    if (job.outputDest != null)
                      _row('Output', job.outputDest!['type'] ?? 'redis'),
                    if (job.tags.isNotEmpty) _row('Tags', job.tags.join(', ')),
                    if (job.skillIds.isNotEmpty)
                      _row('Skills', job.skillIds.join(', ')),
                    if (job.retryCount > 0)
                      _row('Retries', job.retryCount.toString()),
                    if (job.error != null)
                      _row('Error', job.error!),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 16),

            // Prompt
            Row(
              children: [
                Text('Prompt',
                    style: Theme.of(context).textTheme.titleMedium),
                const SizedBox(width: 8),
                IconButton(
                  icon: const Icon(Icons.copy, size: 18),
                  tooltip: 'Copy prompt',
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: job.prompt));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                          content: Text('Prompt copied'),
                          duration: Duration(seconds: 1)),
                    );
                  },
                ),
              ],
            ),
            const SizedBox(height: 8),
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: SelectableText(job.prompt,
                    style: const TextStyle(fontFamily: 'monospace')),
              ),
            ),
            const SizedBox(height: 16),

            // Result / Error / Running state
            _buildResultSection(job),

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

            // Log viewer
            _buildLogViewer(),
          ],
        ),
      ),
    );
  }

  Widget _buildResultSection(Job job) {
    if (job.status == 'running' || job.status == 'pending') {
      return Padding(
        padding: const EdgeInsets.only(bottom: 16),
        child: Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              children: [
                const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2)),
                const SizedBox(width: 12),
                Text('Job is ${job.status}...',
                    style: Theme.of(context).textTheme.bodyLarge),
              ],
            ),
          ),
        ),
      );
    }

    if (job.status == 'failed' && job.error != null) {
      return Padding(
        padding: const EdgeInsets.only(bottom: 16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Error', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Card(
              color: Theme.of(context).colorScheme.errorContainer,
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: SelectableText(
                  job.error!,
                  style: TextStyle(
                    fontFamily: 'monospace',
                    color: Theme.of(context).colorScheme.onErrorContainer,
                  ),
                ),
              ),
            ),
          ],
        ),
      );
    }

    if (_result == null) return const SizedBox.shrink();

    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text('Result',
                  style: Theme.of(context).textTheme.titleMedium),
              const Spacer(),
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                tooltip: 'Copy result',
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: _result!.result));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: Text('Result copied'),
                        duration: Duration(seconds: 1)),
                  );
                },
              ),
              TextButton.icon(
                icon: const Icon(Icons.arrow_forward, size: 16),
                label: const Text('Use in New Job'),
                onPressed: () {
                  context.go(
                      '/jobs/new?prefill_result=${Uri.encodeComponent(_result!.result.length > 5000 ? _result!.result.substring(0, 5000) : _result!.result)}'
                      '&workspace_id=${job.workspaceId ?? ""}'
                      '&model=${job.model ?? ""}');
                },
              ),
            ],
          ),
          const SizedBox(height: 8),
          Card(
            color: Theme.of(context).colorScheme.primaryContainer,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: SelectableText(
                _result!.result.isEmpty
                    ? '(empty result)'
                    : _result!.result,
                style: const TextStyle(fontFamily: 'monospace'),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildLogViewer() {
    final entries = _parseLogEntries();
    if (entries.isEmpty && _rawLogs.isEmpty) {
      return const SizedBox.shrink();
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Activity (${entries.length} entries)',
            style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 8),
        Card(
          color: const Color(0xFF1E1E2E),
          child: SizedBox(
            height: 400,
            child: SingleChildScrollView(
              controller: _logScrollController,
              padding: const EdgeInsets.all(12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: entries,
              ),
            ),
          ),
        ),
      ],
    );
  }

  List<Widget> _parseLogEntries() {
    final widgets = <Widget>[];
    for (final raw in _rawLogs) {
      try {
        final val = json.decode(raw);
        final type = val['type'] as String?;

        if (type == 'assistant') {
          final content =
              val['message']?['content'] as List<dynamic>? ?? [];
          for (final item in content) {
            final ctype = item['type'] as String?;
            if (ctype == 'text') {
              final text = item['text'] as String? ?? '';
              if (text.isNotEmpty) {
                widgets.add(Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Text(
                    text,
                    style: const TextStyle(
                      color: Colors.white,
                      fontSize: 13,
                      height: 1.4,
                    ),
                  ),
                ));
              }
            } else if (ctype == 'tool_use') {
              final name = item['name'] as String? ?? '?';
              final input = item['input'] as Map<String, dynamic>? ?? {};
              final summary = toolSummaryShort(name, input);
              widgets.add(Padding(
                padding: const EdgeInsets.only(bottom: 4),
                child: Text(
                  '> $name $summary',
                  style: TextStyle(
                    color: Colors.blue[300],
                    fontFamily: 'monospace',
                    fontSize: 12,
                  ),
                ),
              ));
            } else if (ctype == 'thinking') {
              final text = item['thinking'] as String? ?? '';
              if (text.isNotEmpty) {
                widgets.add(Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: Text(
                    text.length > 200
                        ? '${text.substring(0, 200)}...'
                        : text,
                    style: TextStyle(
                      color: Colors.grey[600],
                      fontStyle: FontStyle.italic,
                      fontSize: 11,
                    ),
                  ),
                ));
              }
            }
          }
        } else if (type == 'user') {
          final content =
              val['message']?['content'] as List<dynamic>? ?? [];
          for (final item in content) {
            if (item['type'] == 'tool_result') {
              var output = item['content']?.toString() ?? '';
              if (output.length > 500) {
                output = '${output.substring(0, 500)}...';
              }
              if (output.isNotEmpty) {
                widgets.add(Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: Text(
                    output,
                    style: TextStyle(
                      color: Colors.grey[400],
                      fontFamily: 'monospace',
                      fontSize: 11,
                    ),
                  ),
                ));
              }
            }
          }
        } else if (type == 'result') {
          final result = val['result'] as String? ?? '';
          final cost = val['total_cost_usd'] as num? ?? 0;
          final dur = val['duration_ms'] as num? ?? 0;
          if (result.isNotEmpty || cost > 0) {
            widgets.add(Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                '--- Result (cost: \$${cost.toStringAsFixed(4)}, ${(dur / 1000).toStringAsFixed(1)}s) ---'
                '${result.isNotEmpty ? '\n$result' : ''}',
                style: TextStyle(
                  color: Colors.green[300],
                  fontFamily: 'monospace',
                  fontSize: 12,
                ),
              ),
            ));
          }
        }
        // Skip 'system' type
      } catch (_) {
        // Non-JSON line — show as-is
        widgets.add(Text(
          raw,
          style: TextStyle(
              color: Colors.grey[500],
              fontFamily: 'monospace',
              fontSize: 11),
        ));
      }
    }
    return widgets;
  }

  Widget _workspaceRow(String wsId) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const SizedBox(
              width: 100,
              child: Text('Workspace:',
                  style: TextStyle(fontWeight: FontWeight.bold))),
          Expanded(
            child: Row(
              children: [
                InkWell(
                  onTap: () => context.go('/workspaces/$wsId'),
                  child: Text(
                    _workspaceName ?? wsId.substring(0, 8),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.primary,
                      decoration: TextDecoration.underline,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                TextButton.icon(
                  icon: const Icon(Icons.folder_open, size: 14),
                  label: const Text('Files'),
                  onPressed: () => context.go('/workspaces/$wsId'),
                  style: TextButton.styleFrom(
                    padding: const EdgeInsets.symmetric(horizontal: 8),
                    minimumSize: Size.zero,
                    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  ),
                ),
              ],
            ),
          ),
        ],
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

  String _formatTimeout(int seconds) {
    if (seconds >= 86400) return '${seconds ~/ 86400}d';
    if (seconds >= 3600) return '${seconds ~/ 3600}h';
    if (seconds >= 60) return '${seconds ~/ 60}m';
    return '${seconds}s';
  }
}
