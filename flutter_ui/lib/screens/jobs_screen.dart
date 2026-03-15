import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/job.dart';
import '../widgets/status_badge.dart';

class JobsScreen extends ConsumerStatefulWidget {
  const JobsScreen({super.key});

  @override
  ConsumerState<JobsScreen> createState() => _JobsScreenState();
}

class _JobsScreenState extends ConsumerState<JobsScreen> {
  List<Job> _jobs = [];
  bool _loading = true;
  String? _statusFilter;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final api = ref.read(apiClientProvider);
      final jobs = await api.listJobs(status: _statusFilter, limit: 50);
      setState(() {
        _jobs = jobs;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
    }
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
              Text('Jobs', style: Theme.of(context).textTheme.headlineMedium),
              const Spacer(),
              // Status filter chips
              ...['all', 'pending', 'running', 'completed', 'failed'].map(
                (s) => Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: FilterChip(
                    label: Text(s),
                    selected: s == 'all'
                        ? _statusFilter == null
                        : _statusFilter == s,
                    onSelected: (_) {
                      setState(() =>
                          _statusFilter = s == 'all' ? null : s);
                      _refresh();
                    },
                  ),
                ),
              ),
              const SizedBox(width: 16),
              IconButton(onPressed: _refresh, icon: const Icon(Icons.refresh)),
              FilledButton.icon(
                onPressed: () => context.go('/jobs/new'),
                icon: const Icon(Icons.add),
                label: const Text('New Job'),
              ),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Expanded(child: Center(child: CircularProgressIndicator()))
          else if (_jobs.isEmpty)
            const Expanded(child: Center(child: Text('No jobs found')))
          else
            Expanded(
              child: ListView.separated(
                itemCount: _jobs.length,
                separatorBuilder: (_, _i) => const Divider(height: 1),
                itemBuilder: (context, i) {
                  final job = _jobs[i];
                  return ListTile(
                    leading: StatusBadge(status: job.status),
                    title: Text(job.promptPreview,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text(job.shortId),
                    trailing: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (job.costUsd != null)
                          Text('\$${job.costUsd!.toStringAsFixed(4)}  '),
                        if (job.durationMs != null)
                          Text(
                              '${(job.durationMs! / 1000).toStringAsFixed(1)}s'),
                      ],
                    ),
                    onTap: () => context.go('/jobs/${job.id}'),
                  );
                },
              ),
            ),
        ],
      ),
    );
  }
}
