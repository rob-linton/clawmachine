import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'screens/dashboard_screen.dart';
import 'screens/jobs_screen.dart';
import 'screens/job_detail_screen.dart';
import 'screens/submit_job_screen.dart';
import 'screens/skills_screen.dart';
import 'screens/skill_detail_screen.dart';
import 'screens/tools_screen.dart';
import 'screens/tool_detail_screen.dart';
import 'screens/credentials_screen.dart';
import 'screens/schedules_screen.dart';
import 'screens/workspaces_screen.dart';
import 'screens/pipelines_screen.dart';
import 'screens/templates_screen.dart';
import 'screens/settings_screen.dart';
import 'screens/login_screen.dart';
import 'services/api_client.dart';
import 'services/event_service.dart';
import 'widgets/app_shell.dart';

const _apiUrl = String.fromEnvironment('API_URL', defaultValue: 'http://localhost:8080');

final apiClientProvider = Provider<ApiClient>((ref) {
  return ApiClient(_apiUrl);
});

final eventServiceProvider = Provider<EventService>((ref) {
  final service = EventService(_apiUrl);
  service.connect();
  ref.onDispose(() => service.dispose());
  return service;
});

/// Tracks whether the user is authenticated.
final isAuthenticatedProvider = StateProvider<bool>((ref) => false);

final _router = GoRouter(
  initialLocation: '/',
  redirect: (context, state) {
    // Let /login through without auth check
    if (state.uri.toString() == '/login') return null;
    // Auth check happens in ClawApp's initState, which sets isAuthenticated
    // and redirects to /login if needed. GoRouter redirect can't access
    // Riverpod state easily, so we handle it at the widget level.
    return null;
  },
  routes: [
    GoRoute(path: '/login', builder: (_, __) => const LoginScreen()),
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
        GoRoute(
            path: '/templates',
            builder: (_, __) => const TemplatesScreen()),
        GoRoute(
            path: '/pipelines',
            builder: (_, __) => const PipelinesScreen()),
        GoRoute(
            path: '/schedules',
            builder: (_, __) => const SchedulesScreen()),
        GoRoute(
            path: '/workspaces',
            builder: (_, __) => const WorkspacesScreen()),
        GoRoute(
          path: '/workspaces/:id',
          builder: (_, state) =>
              WorkspaceDetailScreen(workspaceId: state.pathParameters['id']!),
        ),
        GoRoute(path: '/skills', builder: (_, __) => const SkillsScreen()),
        GoRoute(
          path: '/skills/:id',
          builder: (_, state) =>
              SkillDetailScreen(skillId: state.pathParameters['id']!),
        ),
        GoRoute(path: '/tools', builder: (_, __) => const ToolsScreen()),
        GoRoute(
          path: '/tools/:id',
          builder: (_, state) =>
              ToolDetailScreen(toolId: state.pathParameters['id']!),
        ),
        GoRoute(path: '/credentials', builder: (_, __) => const CredentialsScreen()),
        GoRoute(
            path: '/settings', builder: (_, __) => const SettingsScreen()),
      ],
    ),
  ],
);

void main() {
  final binding = WidgetsFlutterBinding.ensureInitialized();
  binding.ensureSemantics();
  runApp(const ProviderScope(child: ClawApp()));
}

class ClawApp extends ConsumerStatefulWidget {
  const ClawApp({super.key});

  @override
  ConsumerState<ClawApp> createState() => _ClawAppState();
}

class _ClawAppState extends ConsumerState<ClawApp> {
  @override
  void initState() {
    super.initState();
    _checkAuth();
  }

  Future<void> _checkAuth() async {
    try {
      final api = ref.read(apiClientProvider);
      await api.checkAuth();
      ref.read(isAuthenticatedProvider.notifier).state = true;
    } catch (_) {
      ref.read(isAuthenticatedProvider.notifier).state = false;
      // Redirect to login after frame renders
      WidgetsBinding.instance.addPostFrameCallback((_) {
        _router.go('/login');
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: 'Claw Machine',
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
