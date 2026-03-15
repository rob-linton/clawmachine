# Flutter UI — Web and Desktop Dashboard

## 1. Overview

The Flutter UI provides a real-time monitoring dashboard for ClaudeCodeClaw. It runs as both a web application (served by the Axum API server) and a native desktop application (macOS/Linux/Windows), sharing a single codebase.

**State management**: Riverpod
**Routing**: go_router
**HTTP client**: dio
**Real-time**: WebSocket via web_socket_channel

## 2. Project Structure

```
flutter_ui/
├── pubspec.yaml
├── analysis_options.yaml
├── web/
│   ├── index.html
│   ├── manifest.json
│   └── favicon.png
├── macos/                          # macOS desktop target
├── linux/                          # Linux desktop target
├── windows/                        # Windows desktop target
└── lib/
    ├── main.dart                   # App entry, ProviderScope, MaterialApp
    ├── router.dart                 # go_router configuration
    ├── theme.dart                  # Light/dark theme definitions
    │
    ├── models/                     # Data classes (immutable, with fromJson)
    │   ├── job.dart
    │   ├── job_status.dart
    │   ├── skill.dart
    │   ├── skill_type.dart
    │   ├── cron_schedule.dart
    │   ├── worker_status.dart
    │   ├── system_stats.dart
    │   ├── output_dest.dart
    │   └── ws_event.dart
    │
    ├── services/                   # I/O layer
    │   ├── api_client.dart         # REST API wrapper (dio)
    │   └── websocket_service.dart  # WebSocket connection management
    │
    ├── providers/                  # Riverpod state management
    │   ├── api_client_provider.dart
    │   ├── websocket_provider.dart
    │   ├── jobs_provider.dart
    │   ├── job_detail_provider.dart
    │   ├── skills_provider.dart
    │   ├── crons_provider.dart
    │   ├── workers_provider.dart
    │   ├── stats_provider.dart
    │   └── settings_provider.dart
    │
    ├── screens/                    # Full-page screens
    │   ├── dashboard_screen.dart
    │   ├── jobs_screen.dart
    │   ├── job_detail_screen.dart
    │   ├── submit_job_screen.dart
    │   ├── skills_screen.dart
    │   ├── skill_editor_screen.dart
    │   ├── crons_screen.dart
    │   ├── cron_editor_screen.dart
    │   └── settings_screen.dart
    │
    └── widgets/                    # Reusable UI components
        ├── app_shell.dart          # Side nav + content area layout
        ├── job_card.dart
        ├── job_status_badge.dart
        ├── log_viewer.dart
        ├── queue_chart.dart
        ├── cost_display.dart
        ├── worker_indicator.dart
        ├── skill_chip.dart
        ├── skill_picker.dart
        ├── prompt_editor.dart
        ├── stat_card.dart
        ├── activity_feed.dart
        ├── empty_state.dart
        ├── error_state.dart
        └── loading_shimmer.dart
```

## 3. Routing

```dart
// router.dart
final router = GoRouter(
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
          builder: (_, state) => JobDetailScreen(
            jobId: state.pathParameters['id']!,
          ),
        ),
        GoRoute(path: '/skills', builder: (_, __) => const SkillsScreen()),
        GoRoute(path: '/skills/new', builder: (_, __) => const SkillEditorScreen()),
        GoRoute(
          path: '/skills/:id',
          builder: (_, state) => SkillEditorScreen(
            skillId: state.pathParameters['id'],
          ),
        ),
        GoRoute(path: '/crons', builder: (_, __) => const CronsScreen()),
        GoRoute(path: '/crons/new', builder: (_, __) => const CronEditorScreen()),
        GoRoute(
          path: '/crons/:id',
          builder: (_, state) => CronEditorScreen(
            cronId: state.pathParameters['id'],
          ),
        ),
        GoRoute(path: '/settings', builder: (_, __) => const SettingsScreen()),
      ],
    ),
  ],
);
```

## 4. App Shell

The AppShell provides the persistent navigation sidebar shared across all screens:

```
┌─────────────────────────────────────────────────────────────────┐
│ ┌──────────────┐ ┌──────────────────────────────────────────┐  │
│ │              │ │                                          │  │
│ │  🔥 Claw     │ │         [Screen Content]                 │  │
│ │              │ │                                          │  │
│ │  Dashboard   │ │                                          │  │
│ │  Jobs        │ │                                          │  │
│ │  Skills      │ │                                          │  │
│ │  Schedules   │ │                                          │  │
│ │              │ │                                          │  │
│ │              │ │                                          │  │
│ │              │ │                                          │  │
│ │              │ │                                          │  │
│ │  ──────────  │ │                                          │  │
│ │  Settings    │ │                                          │  │
│ │              │ │                                          │  │
│ │  Workers: 2  │ │                                          │  │
│ │  Queue:  5   │ │                                          │  │
│ │              │ │                                          │  │
│ └──────────────┘ └──────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

The sidebar shows:
- Navigation links with active state highlighting
- Live worker count and queue depth (from WebSocket stats)
- Collapses to icons on narrow viewports (responsive)

## 5. Screen Designs

### 5.1 Dashboard Screen (`/`)

```
┌─────────────────────────────────────────────────────────────┐
│                        Dashboard                             │
│                                                              │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌──────────┐ │
│  │  Pending   │ │  Running   │ │ Completed  │ │   Cost   │ │
│  │     5      │ │     2      │ │    47      │ │  $12.34  │ │
│  │   +2 ↑    │ │            │ │  today     │ │  today   │ │
│  └────────────┘ └────────────┘ └────────────┘ └──────────┘ │
│                                                              │
│  Queue Distribution                                          │
│  ██████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
│  ▲ pending(5)   ▲ running(2)   ▲ completed(47)  ▲ failed(3) │
│                                                              │
│  Workers                                                     │
│  ● worker-1-t0  busy  "Review the code..."       2m         │
│  ○ worker-1-t1  idle                                         │
│  ● worker-2-t0  busy  "Refactor database..."     5m         │
│  ○ worker-2-t1  idle                                         │
│                                                              │
│  Recent Activity                                             │
│  22:35  f47ac10b  ● running   "Review the code..."          │
│  22:33  a1b2c3d4  ✓ completed "Refactor database..."  $0.42 │
│  22:30  e5f6a7b8  ◌ pending   "Analyze test coverage..."    │
│  22:28  b3c4d5e6  ✓ completed "Update documentation"  $0.21 │
│  22:25  c4d5e6f7  ✗ failed    "Deploy to staging" (timeout) │
│                                                              │
│  Quick Submit                                                │
│  ┌─────────────────────────────────────┐ ┌────────────────┐ │
│  │ Enter a prompt...                   │ │    Submit      │ │
│  └─────────────────────────────────────┘ └────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Key behaviors**:
- All stat cards update in real-time via WebSocket `stats` events
- Recent activity feed auto-updates via WebSocket `job_update` events
- Worker indicators show live busy/idle state
- Clicking a job in the activity feed navigates to its detail page
- Quick submit creates a job with default settings

### 5.2 Jobs Screen (`/jobs`)

```
┌─────────────────────────────────────────────────────────────┐
│  Jobs                                          [+ New Job]   │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Status: [All ▾]  Source: [All ▾]  Tags: [________]     │ │
│  │ Search: [________________________]  Sort: [Newest ▾]   │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ ● f47ac10b  RUNNING                          P8  2m    │ │
│  │   Review this PR for security issues                    │ │
│  │   [pr-review] [security]  worker-1-task-0  sonnet      │ │
│  ├─────────────────────────────────────────────────────────┤ │
│  │ ✓ a1b2c3d4  COMPLETED                   $0.42  5m ago  │ │
│  │   Refactor database module to use connection pooling    │ │
│  │   [refactor] [rust]  worker-1-task-1  sonnet            │ │
│  ├─────────────────────────────────────────────────────────┤ │
│  │ ◌ e5f6a7b8  PENDING                          P5  8m    │ │
│  │   Analyze test coverage and suggest improvements        │ │
│  │   [testing] [quality]                                   │ │
│  ├─────────────────────────────────────────────────────────┤ │
│  │ ✗ c4d5e6f7  FAILED                          15m ago    │ │
│  │   Deploy to staging environment                         │ │
│  │   Error: Job timed out after 30m                        │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Showing 1-20 of 54     [< Prev]  1  2  3  [Next >]        │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Key behaviors**:
- Filters are applied immediately (debounced search)
- New jobs appear at the top in real-time via WebSocket
- Status badges update in real-time (pending → running → completed)
- Click any job card to navigate to detail

### 5.3 Job Detail Screen (`/jobs/:id`)

```
┌─────────────────────────────────────────────────────────────┐
│  ← Back to Jobs                                              │
│                                                              │
│  Job f47ac10b-58cc-4372-a567-0e02b2c3d479                   │
│  ● RUNNING                              [Cancel] [Re-submit] │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────────────┐ │
│  │ Created  22:30:00    │  │ Model    sonnet              │ │
│  │ Started  22:30:02    │  │ Budget   $2.00               │ │
│  │ Duration 2m 13s...   │  │ Cost     $0.42 (so far)      │ │
│  │ Source   cli         │  │ Priority 8                    │ │
│  │ Worker   w1-task-0   │  │ Retries  0                   │ │
│  └──────────────────────┘  └──────────────────────────────┘ │
│                                                              │
│  Prompt                                                      │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Review this PR for security issues                      │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Skills: [code-review] [security-audit]                      │
│  Tags:   [pr-review] [security]                              │
│  Output: webhook → https://hooks.slack.com/...               │
│                                                              │
│  Logs                                           [Auto-scroll] │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ 22:30:02  I'll start by reading the source files...     │ │
│  │ 22:30:03  > Read src/main.rs                            │ │
│  │ 22:30:04  (142 lines)                                   │ │
│  │ 22:30:07  I found several security concerns in the      │ │
│  │           codebase. Let me check for SQL injection...    │ │
│  │ 22:30:08  > Grep "sql" in src/                          │ │
│  │ 22:30:09  (3 matches)                                   │ │
│  │ 22:30:12  > Read src/db/queries.rs                      │ │
│  │ ...                                                     │ │
│  │ █  (streaming...)                                       │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Result                                                      │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ (waiting for completion...)                             │ │
│  │                                                         │ │
│  │ -- or when completed: --                                │ │
│  │                                                         │ │
│  │ ## Security Review                                      │ │
│  │                                                         │ │
│  │ ### Critical Issues                                     │ │
│  │ 1. **SQL Injection** (line 42)...                       │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Key behaviors**:
- Log viewer auto-scrolls as new lines arrive (toggle to disable)
- Log lines are streamed in real-time via WebSocket `job_log` subscription
- Status badge, duration, and cost update live
- Result section appears once the job completes (rendered as markdown)
- Cancel button sends cancel request; Re-submit creates a new job with same config

### 5.4 Submit Job Screen (`/jobs/new`)

```
┌─────────────────────────────────────────────────────────────┐
│  Submit New Job                                              │
│                                                              │
│  Prompt *                                                    │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │                                                         │ │
│  │ (multi-line text area, monospace font)                   │ │
│  │                                                         │ │
│  │                                                         │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Skills                                                      │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ [code-review ×] [rust-project ×]  [+ Add skill...]     │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────────────┐ │
│  │ Working Directory    │  │ Model                        │ │
│  │ [/repos/my-project ] │  │ [sonnet ▾]                   │ │
│  └──────────────────────┘  └──────────────────────────────┘ │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────────────┐ │
│  │ Max Budget (USD)     │  │ Priority                     │ │
│  │ [1.00              ] │  │ [═══════●═══] 7              │ │
│  └──────────────────────┘  └──────────────────────────────┘ │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────────────┐ │
│  │ Timeout (seconds)    │  │ Output Destination           │ │
│  │ [1800              ] │  │ ○ Redis (default)            │ │
│  └──────────────────────┘  │ ○ File: [/output       ]    │ │
│                             │ ○ Webhook: [https://...  ]  │ │
│                             └──────────────────────────────┘ │
│                                                              │
│  Tags                                                        │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ [security ×] [automated ×]  [+ Add tag...]             │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│                                        [Cancel]  [Submit]    │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Key behaviors**:
- Skill picker shows searchable dropdown of available skills with type badges
- Model dropdown populated from known models
- Priority slider with numeric display
- Output destination conditionally shows path/URL input
- Tag input supports autocomplete from existing tags
- Submit button posts to API, then navigates to the new job's detail page

### 5.5 Skills Screen (`/skills`)

Grid layout showing skill cards:

```
┌─────────────────────────────────────────────────────────────┐
│  Skills                                      [+ New Skill]   │
│                                                              │
│  Filter: [All types ▾]  [_________ search]                  │
│                                                              │
│  ┌───────────────────┐ ┌───────────────────┐ ┌────────────┐ │
│  │ Code Review       │ │ Security Audit    │ │ Rust       │ │
│  │ TEMPLATE          │ │ TEMPLATE          │ │ CONFIG     │ │
│  │                   │ │                   │ │            │ │
│  │ Structured code   │ │ OWASP-focused     │ │ Rust       │ │
│  │ review criteria   │ │ security review   │ │ project    │ │
│  │                   │ │                   │ │ conventions│ │
│  │ [review][quality] │ │ [security][review]│ │ [rust]     │ │
│  └───────────────────┘ └───────────────────┘ └────────────┘ │
│                                                              │
│  ┌───────────────────┐ ┌───────────────────┐ ┌────────────┐ │
│  │ Run Tests         │ │ TypeScript        │ │ JSON Out   │ │
│  │ SCRIPT            │ │ CONFIG            │ │ TEMPLATE   │ │
│  │                   │ │                   │ │            │ │
│  │ Generic test      │ │ TypeScript        │ │ Force JSON │ │
│  │ runner            │ │ project config    │ │ output     │ │
│  │                   │ │                   │ │ format     │ │
│  │ [testing]         │ │ [typescript]      │ │ [format]   │ │
│  └───────────────────┘ └───────────────────┘ └────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

Click any card → Skill Editor with that skill loaded.

### 5.6 Skill Editor Screen (`/skills/:id` or `/skills/new`)

```
┌─────────────────────────────────────────────────────────────┐
│  ← Back to Skills                                            │
│                                                              │
│  Edit Skill: Code Review                      [Delete]       │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────────────┐ │
│  │ ID (slug) *          │  │ Name *                       │ │
│  │ [code-review       ] │  │ [Code Review              ]  │ │
│  └──────────────────────┘  └──────────────────────────────┘ │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────────────┐ │
│  │ Type *               │  │ Description                  │ │
│  │ [Template ▾]         │  │ [Structured code review...] │ │
│  └──────────────────────┘  └──────────────────────────────┘ │
│                                                              │
│  Tags                                                        │
│  [review ×] [quality ×]  [+ Add...]                         │
│                                                              │
│  Content                                                     │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ When reviewing code, evaluate the following dimensions: │ │
│  │                                                         │ │
│  │ 1. **Correctness**: Does the code do what it claims?    │ │
│  │    Are there edge cases that aren't handled?            │ │
│  │                                                         │ │
│  │ 2. **Security**: Any OWASP Top 10 vulnerabilities?      │ │
│  │    Is user input properly validated and sanitized?      │ │
│  │                                                         │ │
│  │ 3. **Performance**: O(n) analysis, unnecessary          │ │
│  │    allocations, blocking I/O in async context?          │ │
│  │                                                         │ │
│  │ (monospace editor with syntax highlighting)             │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│                                        [Cancel]  [Save]      │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 5.7 Crons Screen (`/crons`)

```
┌─────────────────────────────────────────────────────────────┐
│  Scheduled Jobs                              [+ New Schedule] │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Morning PR Review                              [ON]    │ │
│  │  0 9 * * MON-FRI  (Weekdays at 9:00 AM)               │ │
│  │  "Review all open PRs and post summaries"              │ │
│  │  Last: Mar 14, 09:00  Next: Mar 16, 09:00             │ │
│  │  Skills: [code-review]         [Edit] [Trigger Now]     │ │
│  ├─────────────────────────────────────────────────────────┤ │
│  │  Nightly Tests                                  [ON]    │ │
│  │  0 2 * * *  (Daily at 2:00 AM)                         │ │
│  │  "Run full test suite and report failures"             │ │
│  │  Last: Mar 15, 02:00  Next: Mar 16, 02:00             │ │
│  │  Skills: [run-tests]           [Edit] [Trigger Now]     │ │
│  ├─────────────────────────────────────────────────────────┤ │
│  │  Weekly Report                                  [OFF]   │ │
│  │  0 17 * * FRI  (Fridays at 5:00 PM)                    │ │
│  │  "Generate weekly summary of all changes"              │ │
│  │  Last: Mar 7, 17:00   Next: —                          │ │
│  │  Skills: [json-output]         [Edit] [Trigger Now]     │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 5.8 Settings Screen (`/settings`)

```
┌─────────────────────────────────────────────────────────────┐
│  Settings                                                    │
│                                                              │
│  Connection                                                  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ API Base URL                                            │ │
│  │ [http://localhost:8080                                ] │ │
│  │                                                         │ │
│  │ WebSocket URL                                           │ │
│  │ [ws://localhost:8080/api/v1/ws                        ] │ │
│  │                                                         │ │
│  │ Status: ● Connected                                     │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Appearance                                                  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Theme:  ○ Light  ● Dark  ○ System                      │ │
│  │                                                         │ │
│  │ Timestamps: ○ Relative (5m ago)  ● Absolute (22:30:00) │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Notifications                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ ☑ Show notification on job completion                   │ │
│  │ ☑ Show notification on job failure                      │ │
│  │ ☐ Play sound on notification                            │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Defaults                                                    │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Default model:  [sonnet ▾]                              │ │
│  │ Default budget: [$1.00     ]                            │ │
│  │ Default output: [Redis ▾]                               │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## 6. State Management (Riverpod)

### 6.1 Provider Architecture

```
                    ┌─────────────────┐
                    │SettingsProvider  │  (persists to shared_preferences)
                    │  apiUrl, wsUrl   │
                    │  theme, defaults │
                    └────────┬────────┘
                             │
                    ┌────────┴────────┐
                    │ApiClientProvider │  (dio instance, configured from settings)
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
    ┌─────────┴──────┐  ┌───┴───────┐  ┌──┴──────────┐
    │ JobsProvider   │  │ Skills    │  │ Crons       │
    │ (list + CRUD)  │  │ Provider  │  │ Provider    │
    └─────────┬──────┘  └───────────┘  └─────────────┘
              │
    ┌─────────┴──────────┐
    │ JobDetailProvider   │  (per-job, includes logs stream)
    └────────────────────┘

    ┌──────────────────┐
    │WebSocketProvider │  (StreamProvider, broadcasts events)
    └────────┬─────────┘
             │ listens
    ┌────────┴──────────┐
    │ StatsProvider     │  (updated from WS stats events)
    │ WorkersProvider   │  (updated from WS + polling)
    └───────────────────┘
```

### 6.2 Key Provider Implementations

```dart
// api_client_provider.dart
final apiClientProvider = Provider<ApiClient>((ref) {
  final settings = ref.watch(settingsProvider);
  return ApiClient(baseUrl: settings.apiUrl);
});

// websocket_provider.dart
final websocketProvider = StreamProvider<WsEvent>((ref) {
  final settings = ref.watch(settingsProvider);
  final service = WebSocketService(settings.wsUrl);

  // Auto-subscribe to jobs and stats
  service.subscribe(WsChannel.jobs);
  service.subscribe(WsChannel.stats);

  ref.onDispose(() => service.dispose());
  return service.events;
});

// jobs_provider.dart
final jobsProvider = AsyncNotifierProvider<JobsNotifier, List<Job>>(() {
  return JobsNotifier();
});

class JobsNotifier extends AsyncNotifier<List<Job>> {
  @override
  Future<List<Job>> build() async {
    final api = ref.read(apiClientProvider);

    // Listen to WebSocket for live updates
    ref.listen(websocketProvider, (_, next) {
      next.whenData((event) {
        if (event is JobUpdateEvent) {
          _handleJobUpdate(event);
        }
      });
    });

    return api.listJobs();
  }

  void _handleJobUpdate(JobUpdateEvent event) {
    state = state.whenData((jobs) {
      return jobs.map((job) {
        if (job.id == event.jobId) {
          return job.copyWith(status: event.status);
        }
        return job;
      }).toList();
    });
  }

  Future<Job> submitJob(SubmitJobRequest request) async {
    final api = ref.read(apiClientProvider);
    final job = await api.submitJob(request);
    state = state.whenData((jobs) => [job, ...jobs]);
    return job;
  }
}

// stats_provider.dart
final statsProvider = StateProvider<SystemStats>((ref) {
  ref.listen(websocketProvider, (_, next) {
    next.whenData((event) {
      if (event is StatsEvent) {
        ref.controller.state = SystemStats.fromEvent(event);
      }
    });
  });

  return SystemStats.empty();
});

// job_detail_provider.dart — parameterized by job ID
final jobDetailProvider = FutureProvider.family<Job, String>((ref, jobId) async {
  final api = ref.read(apiClientProvider);
  return api.getJob(jobId);
});

final jobLogsProvider = StreamProvider.family<String, String>((ref, jobId) {
  final ws = ref.read(websocketServiceProvider);
  ws.subscribe(WsChannel.jobLogs(jobId));

  ref.onDispose(() => ws.unsubscribe(WsChannel.jobLogs(jobId)));

  return ws.events
      .where((e) => e is JobLogEvent && e.jobId == jobId)
      .map((e) => (e as JobLogEvent).line);
});
```

## 7. WebSocket Service

```dart
class WebSocketService {
  final String url;
  WebSocketChannel? _channel;
  final _controller = StreamController<WsEvent>.broadcast();
  Timer? _reconnectTimer;
  int _reconnectAttempts = 0;

  WebSocketService(this.url);

  Stream<WsEvent> get events => _controller.stream;

  void connect() {
    _channel = WebSocketChannel.connect(Uri.parse(url));
    _reconnectAttempts = 0;

    _channel!.stream.listen(
      (data) {
        final json = jsonDecode(data as String);
        final event = WsEvent.fromJson(json);
        _controller.add(event);
      },
      onDone: _reconnect,
      onError: (_) => _reconnect(),
    );
  }

  void subscribe(WsChannel channel) {
    _send({'type': 'subscribe', ...channel.toJson()});
  }

  void unsubscribe(WsChannel channel) {
    _send({'type': 'unsubscribe', ...channel.toJson()});
  }

  void _send(Map<String, dynamic> message) {
    _channel?.sink.add(jsonEncode(message));
  }

  void _reconnect() {
    _reconnectAttempts++;
    final delay = Duration(
      seconds: min(pow(2, _reconnectAttempts).toInt(), 30),
    );
    _reconnectTimer = Timer(delay, connect);
  }

  void dispose() {
    _reconnectTimer?.cancel();
    _channel?.sink.close();
    _controller.close();
  }
}
```

## 8. Data Models (Dart)

```dart
// job.dart
@immutable
class Job {
  final String id;
  final JobStatus status;
  final String prompt;
  final List<String> skillIds;
  final String? workingDir;
  final String? model;
  final double? maxBudgetUsd;
  final OutputDest outputDest;
  final String source;
  final int priority;
  final List<String> tags;
  final DateTime createdAt;
  final DateTime? startedAt;
  final DateTime? completedAt;
  final String? workerId;
  final String? error;
  final double? costUsd;
  final int? durationMs;
  final int retryCount;

  // fromJson, toJson, copyWith...
}

enum JobStatus { pending, running, completed, failed, cancelled }

// skill.dart
@immutable
class Skill {
  final String id;
  final String name;
  final SkillType skillType;
  final String content;
  final String description;
  final List<String> tags;
  final DateTime createdAt;
  final DateTime updatedAt;
}

enum SkillType { template, claudeConfig, script }
```

## 9. Platform Considerations

### 9.1 Web

- Served as static files by the Axum API server
- WebSocket connects to the same host
- No CORS issues in production (same origin)
- Development: Flutter dev server on :3000, API on :8080 (CORS needed)

### 9.2 Desktop (macOS/Linux/Windows)

- Connects to a configurable API URL (settings screen)
- Window title shows connection status
- System tray integration (future) for notifications
- Menu bar shortcuts for common actions

### 9.3 Responsive Layout

- Sidebar collapses to icon rail below 768px width
- Job cards stack vertically on narrow screens
- Dashboard stat cards wrap to 2x2 grid on narrow screens
- Log viewer goes full-width on narrow screens

## 10. Dependencies (pubspec.yaml)

```yaml
dependencies:
  flutter:
    sdk: flutter
  flutter_riverpod: ^2.5.0
  riverpod_annotation: ^2.3.0
  go_router: ^14.0.0
  dio: ^5.4.0
  web_socket_channel: ^3.0.0
  json_annotation: ^4.9.0
  intl: ^0.19.0
  shared_preferences: ^2.3.0
  google_fonts: ^6.2.0
  fl_chart: ^0.69.0          # For queue distribution chart
  flutter_markdown: ^0.7.0    # For rendering job results
  shimmer: ^3.0.0             # Loading states
  url_launcher: ^6.3.0

dev_dependencies:
  flutter_test:
    sdk: flutter
  build_runner: ^2.4.0
  json_serializable: ^6.8.0
  riverpod_generator: ^2.4.0
  flutter_lints: ^4.0.0
  mocktail: ^1.0.0
```
