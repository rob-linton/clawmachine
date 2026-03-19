class CronSchedule {
  final String id;
  final String name;
  final String schedule;
  final bool enabled;
  final String prompt;
  final List<String> skillIds;
  final List<String> toolIds;
  final String workingDir;
  final String? model;
  final double? maxBudgetUsd;
  final Map<String, dynamic>? outputDest;
  final List<String> tags;
  final int priority;
  final String? workspaceId;
  final String? templateId;
  final String? lastRun;
  final String? lastJobId;
  final String createdAt;

  CronSchedule({
    required this.id,
    required this.name,
    required this.schedule,
    this.enabled = true,
    required this.prompt,
    this.skillIds = const [],
    this.toolIds = const [],
    this.workingDir = '.',
    this.model,
    this.maxBudgetUsd,
    this.outputDest,
    this.tags = const [],
    this.priority = 5,
    this.workspaceId,
    this.templateId,
    this.lastRun,
    this.lastJobId,
    required this.createdAt,
  });

  factory CronSchedule.fromJson(Map<String, dynamic> json) => CronSchedule(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        schedule: json['schedule'] ?? '',
        enabled: json['enabled'] ?? true,
        prompt: json['prompt'] ?? '',
        skillIds: List<String>.from(json['skill_ids'] ?? []),
        toolIds: List<String>.from(json['tool_ids'] ?? []),
        workingDir: json['working_dir'] ?? '.',
        model: json['model'],
        maxBudgetUsd: (json['max_budget_usd'] as num?)?.toDouble(),
        outputDest: json['output_dest'] is Map<String, dynamic>
            ? json['output_dest']
            : null,
        tags: List<String>.from(json['tags'] ?? []),
        priority: json['priority'] ?? 5,
        workspaceId: json['workspace_id'],
        templateId: json['template_id'],
        lastRun: json['last_run'],
        lastJobId: json['last_job_id'],
        createdAt: json['created_at'] ?? '',
      );

  Map<String, dynamic> toCreateJson() => {
        'name': name,
        'schedule': schedule,
        'enabled': enabled,
        'prompt': prompt,
        if (skillIds.isNotEmpty) 'skill_ids': skillIds,
        if (toolIds.isNotEmpty) 'tool_ids': toolIds,
        if (workingDir != '.') 'working_dir': workingDir,
        if (model != null) 'model': model,
        if (maxBudgetUsd != null) 'max_budget_usd': maxBudgetUsd,
        if (outputDest != null) 'output_dest': outputDest,
        if (tags.isNotEmpty) 'tags': tags,
        'priority': priority,
      };
}
