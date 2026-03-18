import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../main.dart';

class AppShell extends ConsumerWidget {
  final Widget child;
  const AppShell({super.key, required this.child});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final location = GoRouterState.of(context).uri.toString();
    final idx = _indexForLocation(location);

    return Scaffold(
      body: Row(
        children: [
          NavigationRail(
            selectedIndex: idx >= 0 ? idx : null,
            labelType: NavigationRailLabelType.all,
            leading: Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Text('Claw',
                  style: Theme.of(context)
                      .textTheme
                      .titleLarge
                      ?.copyWith(fontWeight: FontWeight.bold)),
            ),
            trailing: Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  IconButton(
                    icon: Icon(Icons.settings,
                        color: location.startsWith('/settings')
                            ? Theme.of(context).colorScheme.primary
                            : null),
                    tooltip: 'Settings',
                    onPressed: () => context.go('/settings'),
                  ),
                  const SizedBox(height: 8),
                  IconButton(
                    icon: const Icon(Icons.logout),
                    tooltip: 'Sign out',
                    onPressed: () async {
                      try {
                        final api = ref.read(apiClientProvider);
                        await api.logout();
                      } catch (_) {}
                      if (context.mounted) context.go('/login');
                    },
                  ),
                  const SizedBox(height: 16),
                ],
              ),
            ),
            destinations: const [
              NavigationRailDestination(
                  icon: Icon(Icons.dashboard), label: Text('Dashboard')),
              NavigationRailDestination(
                  icon: Icon(Icons.work), label: Text('Jobs')),
              NavigationRailDestination(
                  icon: Icon(Icons.description), label: Text('Templates')),
              NavigationRailDestination(
                  icon: Icon(Icons.view_list), label: Text('Pipelines')),
              NavigationRailDestination(
                  icon: Icon(Icons.schedule), label: Text('Schedules')),
              NavigationRailDestination(
                  icon: Icon(Icons.folder_open), label: Text('Workspaces')),
              NavigationRailDestination(
                  icon: Icon(Icons.auto_fix_high), label: Text('Skills')),
            ],
            onDestinationSelected: (i) {
              switch (i) {
                case 0:
                  context.go('/');
                case 1:
                  context.go('/jobs');
                case 2:
                  context.go('/templates');
                case 3:
                  context.go('/pipelines');
                case 4:
                  context.go('/schedules');
                case 5:
                  context.go('/workspaces');
                case 6:
                  context.go('/skills');
              }
            },
          ),
          const VerticalDivider(width: 1),
          Expanded(child: child),
        ],
      ),
    );
  }

  int _indexForLocation(String location) {
    if (location.startsWith('/jobs')) return 1;
    if (location.startsWith('/templates')) return 2;
    if (location.startsWith('/pipelines')) return 3;
    if (location.startsWith('/schedules')) return 4;
    if (location.startsWith('/workspaces')) return 5;
    if (location.startsWith('/skills')) return 6;
    if (location.startsWith('/settings')) return -1;
    return 0;
  }
}
