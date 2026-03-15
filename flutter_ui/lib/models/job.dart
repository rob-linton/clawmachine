class Job {
  final String id;
  final String status;
  final String prompt;
  final List<String> skillIds;
  final String? model;
  final String? workerId;
  final String? error;
  final double? costUsd;
  final int? durationMs;
  final String createdAt;
  final String? startedAt;
  final String? completedAt;
  final List<String> tags;
  final int priority;

  Job({
    required this.id,
    required this.status,
    required this.prompt,
    this.skillIds = const [],
    this.model,
    this.workerId,
    this.error,
    this.costUsd,
    this.durationMs,
    required this.createdAt,
    this.startedAt,
    this.completedAt,
    this.tags = const [],
    this.priority = 5,
  });

  factory Job.fromJson(Map<String, dynamic> json) => Job(
        id: json['id'] ?? '',
        status: json['status'] ?? 'unknown',
        prompt: json['prompt'] ?? '',
        skillIds: List<String>.from(json['skill_ids'] ?? []),
        model: json['model'],
        workerId: json['worker_id'],
        error: json['error'],
        costUsd: (json['cost_usd'] as num?)?.toDouble(),
        durationMs: json['duration_ms'] as int?,
        createdAt: json['created_at'] ?? '',
        startedAt: json['started_at'],
        completedAt: json['completed_at'],
        tags: List<String>.from(json['tags'] ?? []),
        priority: json['priority'] ?? 5,
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
