class Job {
  final String id;
  final String status;
  final String prompt;
  final List<String> skillIds;
  final List<String> skillTags;
  final String workingDir;
  final String? model;
  final double? maxBudgetUsd;
  final List<String>? allowedTools;
  final Map<String, dynamic>? outputDest;
  final String? source;
  final int priority;
  final List<String> tags;
  final String createdAt;
  final String? startedAt;
  final String? completedAt;
  final String? workerId;
  final String? error;
  final double? costUsd;
  final int? durationMs;
  final int retryCount;
  final int? timeoutSecs;
  final String? cronId;
  final dynamic skillSnapshot;
  final String? assembledPrompt;

  Job({
    required this.id,
    required this.status,
    required this.prompt,
    this.skillIds = const [],
    this.skillTags = const [],
    this.workingDir = '.',
    this.model,
    this.maxBudgetUsd,
    this.allowedTools,
    this.outputDest,
    this.source,
    this.priority = 5,
    this.tags = const [],
    required this.createdAt,
    this.startedAt,
    this.completedAt,
    this.workerId,
    this.error,
    this.costUsd,
    this.durationMs,
    this.retryCount = 0,
    this.timeoutSecs,
    this.cronId,
    this.skillSnapshot,
    this.assembledPrompt,
  });

  factory Job.fromJson(Map<String, dynamic> json) => Job(
        id: json['id'] ?? '',
        status: json['status'] ?? 'unknown',
        prompt: json['prompt'] ?? '',
        skillIds: List<String>.from(json['skill_ids'] ?? []),
        skillTags: List<String>.from(json['skill_tags'] ?? []),
        workingDir: json['working_dir'] ?? '.',
        model: json['model'],
        maxBudgetUsd: (json['max_budget_usd'] as num?)?.toDouble(),
        allowedTools: json['allowed_tools'] != null
            ? List<String>.from(json['allowed_tools'])
            : null,
        outputDest: json['output_dest'] is Map<String, dynamic>
            ? json['output_dest']
            : null,
        source: json['source'],
        priority: json['priority'] ?? 5,
        tags: List<String>.from(json['tags'] ?? []),
        createdAt: json['created_at'] ?? '',
        startedAt: json['started_at'],
        completedAt: json['completed_at'],
        workerId: json['worker_id'],
        error: json['error'],
        costUsd: (json['cost_usd'] as num?)?.toDouble(),
        durationMs: json['duration_ms'] as int?,
        retryCount: json['retry_count'] ?? 0,
        timeoutSecs: json['timeout_secs'] as int?,
        cronId: json['cron_id'],
        skillSnapshot: json['skill_snapshot'],
        assembledPrompt: json['assembled_prompt'],
      );

  String get shortId => id.length >= 8 ? id.substring(0, 8) : id;

  String get promptPreview =>
      prompt.length > 80 ? '${prompt.substring(0, 80)}...' : prompt;
}

class JobResult {
  final String jobId;
  final String result;
  final double costUsd;
  final int durationMs;

  JobResult({
    required this.jobId,
    required this.result,
    required this.costUsd,
    required this.durationMs,
  });

  factory JobResult.fromJson(Map<String, dynamic> json) => JobResult(
        jobId: json['job_id'] ?? '',
        result: json['result'] ?? '',
        costUsd: (json['cost_usd'] as num?)?.toDouble() ?? 0,
        durationMs: json['duration_ms'] as int? ?? 0,
      );
}

class QueueStatus {
  final int pending;
  final int running;
  final int completed;
  final int failed;

  QueueStatus({
    this.pending = 0,
    this.running = 0,
    this.completed = 0,
    this.failed = 0,
  });

  factory QueueStatus.fromJson(Map<String, dynamic> json) => QueueStatus(
        pending: json['pending'] ?? 0,
        running: json['running'] ?? 0,
        completed: json['completed'] ?? 0,
        failed: json['failed'] ?? 0,
      );
}
