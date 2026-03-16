class JobTemplate {
  final String id;
  final String name;
  final String description;
  final String prompt;
  final List<String> skillIds;
  final String? workspaceId;
  final String? model;
  final int? timeoutSecs;
  final int priority;
  final List<String> tags;
  final String createdAt;
  final String updatedAt;

  JobTemplate({
    required this.id,
    required this.name,
    this.description = '',
    required this.prompt,
    this.skillIds = const [],
    this.workspaceId,
    this.model,
    this.timeoutSecs,
    this.priority = 5,
    this.tags = const [],
    required this.createdAt,
    required this.updatedAt,
  });

  factory JobTemplate.fromJson(Map<String, dynamic> json) => JobTemplate(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        description: json['description'] ?? '',
        prompt: json['prompt'] ?? '',
        skillIds: List<String>.from(json['skill_ids'] ?? []),
        workspaceId: json['workspace_id'],
        model: json['model'],
        timeoutSecs: json['timeout_secs'] as int?,
        priority: json['priority'] ?? 5,
        tags: List<String>.from(json['tags'] ?? []),
        createdAt: json['created_at'] ?? '',
        updatedAt: json['updated_at'] ?? '',
      );
}
