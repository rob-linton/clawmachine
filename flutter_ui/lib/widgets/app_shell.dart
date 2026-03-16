import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

class AppShell extends StatelessWidget {
  final Widget child;
  const AppShell({super.key, required this.child});

  @override
  Widget build(BuildContext context) {
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
              child: Align(
                alignment: Alignment.bottomCenter,
                child: Padding(
                  padding: const EdgeInsets.only(bottom: 16),
                  child: IconButton(
                    icon: Icon(Icons.settings,
                        color: location.startsWith('/settings')
                            ? Theme.of(context).colorScheme.primary
                            : null),
                    tooltip: 'Settings',
                    onPressed: () => context.go('/settings'),
                  ),
                ),
              ),
            ),
            destinations: const [
              NavigationRailDestination(
                  icon: Icon(Icons.dashboard), label: Text('Dashboard')),
              NavigationRailDestination(
                  icon: Icon(Icons.work), label: Text('Jobs')),
              NavigationRailDestination(
                  icon: Icon(Icons.schedule), label: Text('Schedules')),
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
                  context.go('/schedules');
                case 3:
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
    if (location.startsWith('/schedules')) return 2;
    if (location.startsWith('/skills')) return 3;
    if (location.startsWith('/settings')) return -1;
    return 0;
  }
}
