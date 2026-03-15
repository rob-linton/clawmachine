import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'screens/dashboard_screen.dart';
import 'screens/jobs_screen.dart';
import 'screens/job_detail_screen.dart';
import 'screens/submit_job_screen.dart';
import 'screens/skills_screen.dart';
import 'services/api_client.dart';
import 'widgets/app_shell.dart';

final apiClientProvider = Provider<ApiClient>((ref) {
  return ApiClient('http://localhost:8080');
});

final _router = GoRouter(
  initialLocation: '/',
  routes: [
    ShellRoute(
      builder: (context, state, child) => AppShell(child: child),
      routes: [
        GoRoute(path: '/', builder: (_, __) => const DashboardScreen()),
        GoRoute(path: '/jobs', builder: (_, __) => const JobsScreen()),
        GoRoute(path: '/jobs/new', builder: (_, __) => const SubmitJobScreen()),
        GoRoute(
          path: '/jobs/:id',
          builder: (_, state) =>
              JobDetailScreen(jobId: state.pathParameters['id']!),
        ),
        GoRoute(path: '/skills', builder: (_, __) => const SkillsScreen()),
      ],
    ),
  ],
);

void main() {
  runApp(const ProviderScope(child: ClawApp()));
}

class ClawApp extends StatelessWidget {
  const ClawApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: 'ClaudeCodeClaw',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorSchemeSeed: Colors.indigo,
        brightness: Brightness.dark,
        useMaterial3: true,
      ),
      routerConfig: _router,
    );
  }
}
