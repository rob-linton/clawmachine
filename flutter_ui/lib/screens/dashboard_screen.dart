import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/job.dart';

class DashboardScreen extends ConsumerStatefulWidget {
  const DashboardScreen({super.key});

  @override
  ConsumerState<DashboardScreen> createState() => _DashboardScreenState();
}

class _DashboardScreenState extends ConsumerState<DashboardScreen> {
  QueueStatus? _status;
  List<Job> _recentJobs = [];
  bool _loading = true;
  StreamSubscription? _eventSub;

  @override
  void initState() {
    super.initState();
    _refresh();
    Timer? debounce;
    _eventSub = ref.read(eventServiceProvider).jobUpdates.listen((_) {
      debounce?.cancel();
      debounce = Timer(const Duration(seconds: 1), () {
        if (mounted) _refresh();
      });
    });
  }

  @override
  void dispose() {
    _eventSub?.cancel();
    super.dispose();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final api = ref.read(apiClientProvider);
      final results = await Future.wait([
        api.getStatus(),
        api.listJobs(limit: 10),
      ]);
      setState(() {
        _status = results[0] as QueueStatus;
        _recentJobs = results[1] as List<Job>;
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
              Text('Dashboard',
                  style: Theme.of(context).textTheme.headlineMedium),
              const Spacer(),
              IconButton(
                  onPressed: _refresh, icon: const Icon(Icons.refresh)),
              const SizedBox(width: 8),
              FilledButton.icon(
                onPressed: () => context.go('/jobs/new'),
                icon: const Icon(Icons.add),
                label: const Text('New Job'),
              ),
            ],
          ),
          const SizedBox(height: 24),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else ...[
            // Stat cards
            Row(
              children: [
                _StatCard('Pending', '${_status?.pending ?? 0}',
                    Colors.orange, Icons.schedule),
                const SizedBox(width: 16),
                _StatCard('Running', '${_status?.running ?? 0}',
                    Colors.blue, Icons.play_circle),
                const SizedBox(width: 16),
                _StatCard('Completed', '${_status?.completed ?? 0}',
                    Colors.green, Icons.check_circle),
                const SizedBox(width: 16),
                _StatCard('Failed', '${_status?.failed ?? 0}', Colors.red,
                    Icons.error),
              ],
            ),
            const SizedBox(height: 32),
            Text('Recent Jobs',
                style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 12),
            Expanded(
              child: _recentJobs.isEmpty
                  ? const Center(child: Text('No jobs yet'))
                  : ListView.builder(
                      itemCount: _recentJobs.length,
                      itemBuilder: (context, i) {
                        final job = _recentJobs[i];
                        return ListTile(
                          leading: _statusIcon(job.status),
                          title: Text(job.promptPreview,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis),
                          subtitle: Text(
                              '${job.shortId} - ${job.status}${job.costUsd != null ? ' - \$${job.costUsd!.toStringAsFixed(4)}' : ''}'),
                          trailing: job.durationMs != null
                              ? Text('${(job.durationMs! / 1000).toStringAsFixed(1)}s')
                              : null,
                          onTap: () => context.go('/jobs/${job.id}'),
                        );
                      },
                    ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _statusIcon(String status) {
    final (color, icon) = switch (status) {
      'pending' => (Colors.orange, Icons.schedule),
      'running' => (Colors.blue, Icons.play_circle),
      'completed' => (Colors.green, Icons.check_circle),
      'failed' => (Colors.red, Icons.error),
      'cancelled' => (Colors.grey, Icons.cancel),
      _ => (Colors.grey, Icons.help),
    };
    return Icon(icon, color: color);
  }
}

class _StatCard extends StatelessWidget {
  final String label;
  final String value;
  final Color color;
  final IconData icon;

  const _StatCard(this.label, this.value, this.color, this.icon);

  @override
  Widget build(BuildContext context) {
    return Expanded(
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            children: [
              Icon(icon, color: color, size: 32),
              const SizedBox(height: 8),
              Text(value,
                  style: Theme.of(context)
                      .textTheme
                      .headlineLarge
                      ?.copyWith(color: color, fontWeight: FontWeight.bold)),
              Text(label, style: Theme.of(context).textTheme.bodySmall),
            ],
          ),
        ),
      ),
    );
  }
}
