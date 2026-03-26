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
    final currentUser = ref.watch(currentUserProvider);
    final username = currentUser?['username'] as String?;
    final role = currentUser?['role'] as String?;
    final isAdmin = role == 'admin';
    final theme = Theme.of(context);

    return Scaffold(
      body: Row(
        children: [
          // Custom scrollable sidebar
          Container(
            width: 88,
            color: theme.colorScheme.surface,
            child: Column(
              children: [
                // Logo (fixed)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 12),
                  child: _ClawLogo(),
                ),
                const Divider(height: 1),
                // Scrollable nav items
                Expanded(
                  child: SingleChildScrollView(
                    child: Column(
                      children: [
                        const SizedBox(height: 8),
                        _NavItem(Icons.dashboard, 'Dashboard', idx == 0, () => context.go('/'), context),
                        _NavItem(Icons.chat, 'Chat', idx == 1, () => context.go('/chat'), context),
                        _NavItem(Icons.work, 'Jobs', idx == 2, () => context.go('/jobs'), context),
                        _NavItem(Icons.description, 'Templates', idx == 3, () => context.go('/templates'), context),
                        _NavItem(Icons.view_list, 'Pipelines', idx == 4, () => context.go('/pipelines'), context),
                        _NavItem(Icons.schedule, 'Schedules', idx == 5, () => context.go('/schedules'), context),
                        _NavItem(Icons.folder_open, 'Workspaces', idx == 6, () => context.go('/workspaces'), context),
                        _NavItem(Icons.auto_fix_high, 'Skills', idx == 7, () => context.go('/skills'), context),
                        _NavItem(Icons.build_circle, 'Tools', idx == 8, () => context.go('/tools'), context),
                        _NavItem(Icons.vpn_key, 'Credentials', idx == 9, () => context.go('/credentials'), context),
                        const SizedBox(height: 8),
                      ],
                    ),
                  ),
                ),
                // Footer (fixed) — user info, settings, logout
                const Divider(height: 1),
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  child: Column(
                    children: [
                      if (isAdmin)
                        IconButton(
                          icon: Icon(Icons.people,
                              color: location.startsWith('/users')
                                  ? theme.colorScheme.primary
                                  : null),
                          tooltip: 'Users',
                          onPressed: () => context.go('/users'),
                        ),
                      IconButton(
                        icon: Icon(Icons.settings,
                            color: location.startsWith('/settings')
                                ? theme.colorScheme.primary
                                : null),
                        tooltip: 'Settings',
                        onPressed: () => context.go('/settings'),
                      ),
                      const SizedBox(height: 4),
                      // Logged-in user
                      if (username != null) ...[
                        Semantics(
                          label: 'Logged in as $username',
                          child: Tooltip(
                            message: '${isAdmin ? "Admin" : "User"}: $username',
                            child: Column(
                              children: [
                                CircleAvatar(
                                  radius: 14,
                                  backgroundColor: isAdmin
                                      ? Colors.orange.shade700
                                      : theme.colorScheme.primary,
                                  child: Text(
                                    username[0].toUpperCase(),
                                    style: const TextStyle(
                                        color: Colors.white,
                                        fontSize: 12,
                                        fontWeight: FontWeight.bold),
                                  ),
                                ),
                                const SizedBox(height: 2),
                                Text(
                                  username,
                                  style: theme.textTheme.bodySmall?.copyWith(fontSize: 10),
                                  overflow: TextOverflow.ellipsis,
                                ),
                              ],
                            ),
                          ),
                        ),
                        const SizedBox(height: 4),
                      ],
                      IconButton(
                        icon: const Icon(Icons.logout, size: 20),
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
                    ],
                  ),
                ),
              ],
            ),
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

/// Single nav item matching NavigationRail style.
class _NavItem extends StatelessWidget {
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;
  final BuildContext parentContext;

  const _NavItem(this.icon, this.label, this.selected, this.onTap, this.parentContext);

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final color = selected ? theme.colorScheme.primary : theme.colorScheme.onSurface.withValues(alpha: 0.7);

    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(12),
      child: Container(
        width: 80,
        padding: const EdgeInsets.symmetric(vertical: 6),
        decoration: selected
            ? BoxDecoration(
                color: theme.colorScheme.primary.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(12),
              )
            : null,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 22, color: color),
            const SizedBox(height: 2),
            Text(
              label,
              style: theme.textTheme.bodySmall?.copyWith(
                fontSize: 10,
                color: color,
                fontWeight: selected ? FontWeight.bold : FontWeight.normal,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
      ),
    );
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
