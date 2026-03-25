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
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: const _ClawLogo(),
            ),
            trailing: Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  if (ref.watch(currentUserProvider)?['role'] == 'admin')
                    IconButton(
                      icon: Icon(Icons.people,
                          color: location.startsWith('/users')
                              ? Theme.of(context).colorScheme.primary
                              : null),
                      tooltip: 'Users',
                      onPressed: () => context.go('/users'),
                    ),
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
                      ref.read(currentUserProvider.notifier).state = null;
                      ref.read(isAuthenticatedProvider.notifier).state = false;
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
                  icon: Icon(Icons.chat), label: Text('Chat')),
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
              NavigationRailDestination(
                  icon: Icon(Icons.build_circle), label: Text('Tools')),
              NavigationRailDestination(
                  icon: Icon(Icons.vpn_key), label: Text('Credentials')),
            ],
            onDestinationSelected: (i) {
              switch (i) {
                case 0:
                  context.go('/');
                case 1:
                  context.go('/chat');
                case 2:
                  context.go('/jobs');
                case 3:
                  context.go('/templates');
                case 4:
                  context.go('/pipelines');
                case 5:
                  context.go('/schedules');
                case 6:
                  context.go('/workspaces');
                case 7:
                  context.go('/skills');
                case 8:
                  context.go('/tools');
                case 9:
                  context.go('/credentials');
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
    if (location.startsWith('/chat')) return 1;
    if (location.startsWith('/jobs')) return 2;
    if (location.startsWith('/templates')) return 3;
    if (location.startsWith('/pipelines')) return 4;
    if (location.startsWith('/schedules')) return 5;
    if (location.startsWith('/workspaces')) return 6;
    if (location.startsWith('/skills')) return 7;
    if (location.startsWith('/tools')) return 8;
    if (location.startsWith('/credentials')) return 9;
    if (location.startsWith('/settings')) return -1;
    return 0;
  }
}

class _ClawLogo extends StatefulWidget {
  const _ClawLogo();

  @override
  State<_ClawLogo> createState() => _ClawLogoState();
}

class _ClawLogoState extends State<_ClawLogo> {
  bool _animating = false;

  void _toggle() {
    if (_animating) {
      setState(() => _animating = false);
      return;
    }
    setState(() => _animating = true);
    Future.delayed(const Duration(seconds: 5), () {
      if (mounted) setState(() => _animating = false);
    });
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: _toggle,
      child: Column(
        children: [
          Image.network(
            _animating ? 'clawmachine_pickup.gif' : 'clawmachine_logo.png',
            height: 40,
            filterQuality: FilterQuality.none,
          ),
          const SizedBox(height: 4),
          Text('Claw\nMachine',
              textAlign: TextAlign.center,
              style: Theme.of(context)
                  .textTheme
                  .titleSmall
                  ?.copyWith(fontWeight: FontWeight.bold, height: 1.2)),
        ],
      ),
    );
  }
}
