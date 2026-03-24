import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';
import '../models/cron_schedule.dart';

class SchedulesScreen extends ConsumerStatefulWidget {
  const SchedulesScreen({super.key});

  @override
  ConsumerState<SchedulesScreen> createState() => _SchedulesScreenState();
}

class _SchedulesScreenState extends ConsumerState<SchedulesScreen> {
  List<CronSchedule> _crons = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final crons = await ref.read(apiClientProvider).listCrons();
      setState(() {
        _crons = crons;
        _loading = false;
      });
    } catch (e) {
      setState(() => _loading = false);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to load schedules: $e')));
      }
    }
  }

  Future<void> _triggerCron(CronSchedule cron) async {
    try {
      final result = await ref.read(apiClientProvider).triggerCron(cron.id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Triggered! Job ${result['job_id']}'),
        ));
      }
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Trigger failed: $e')));
      }
    }
  }

  Future<void> _deleteCron(CronSchedule cron) async {
    final confirm = await showDialog<bool>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete Schedule'),
        content: Text('Delete "${cron.name}"?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Delete')),
        ],
      ),
    );
    if (confirm != true) return;
    try {
      await ref.read(apiClientProvider).deleteCron(cron.id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
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
              Semantics(header: true, label: 'Schedules', child: Text('Schedules', style: Theme.of(context).textTheme.headlineMedium)),
              const Spacer(),
              FilledButton.icon(
                onPressed: () => context.go('/schedules/create'),
                icon: const Icon(Icons.add),
                label: const Text('New Schedule'),
              ),
              const SizedBox(width: 8),
              IconButton(onPressed: _refresh, icon: const Icon(Icons.refresh)),
            ],
          ),
          const SizedBox(height: 16),
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else if (_crons.isEmpty)
            const Center(
              child: Padding(
                padding: EdgeInsets.all(48),
                child: Text('No schedules yet. Create one to run jobs on a cron.'),
              ),
            )
          else
            Expanded(
              child: ListView.builder(
                itemCount: _crons.length,
                itemBuilder: (context, i) => _buildCronTile(_crons[i]),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildCronTile(CronSchedule cron) {
    return Card(
      child: ListTile(
        title: Row(
          children: [
            Semantics(label: 'Schedule ${cron.name}', child: Text(cron.name, style: const TextStyle(fontWeight: FontWeight.bold))),
            const SizedBox(width: 8),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
              decoration: BoxDecoration(
                color: cron.enabled
                    ? Colors.green.withValues(alpha: 0.2)
                    : Colors.grey.withValues(alpha: 0.2),
                borderRadius: BorderRadius.circular(4),
              ),
              child: Text(
                cron.enabled ? 'enabled' : 'disabled',
                style: TextStyle(
                  fontSize: 12,
                  color: cron.enabled ? Colors.green : Colors.grey,
                ),
              ),
            ),
          ],
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 4),
            Text(cron.schedule, style: const TextStyle(fontFamily: 'monospace')),
            Text(cron.prompt.length > 80
                ? '${cron.prompt.substring(0, 80)}...'
                : cron.prompt),
            if (cron.lastRun != null) Text('Last run: ${cron.lastRun}', style: const TextStyle(fontSize: 12)),
          ],
        ),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              icon: const Icon(Icons.play_arrow),
              tooltip: 'Trigger Now',
              onPressed: () => _triggerCron(cron),
            ),
            IconButton(
              icon: const Icon(Icons.edit),
              tooltip: 'Edit',
              onPressed: () => context.go('/schedules/${cron.id}/edit'),
            ),
            IconButton(
              icon: const Icon(Icons.delete),
              tooltip: 'Delete',
              onPressed: () => _deleteCron(cron),
            ),
          ],
        ),
      ),
    );
  }
}
