class Workspace {
  final String id;
  final String name;
  final String description;
  final String path;
  final List<String> skillIds;
  final String? claudeMd;
  final String createdAt;
  final String updatedAt;

  Workspace({
    required this.id,
    required this.name,
    this.description = '',
    required this.path,
    this.skillIds = const [],
    this.claudeMd,
    required this.createdAt,
    required this.updatedAt,
  });

  factory Workspace.fromJson(Map<String, dynamic> json) => Workspace(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        description: json['description'] ?? '',
        path: json['path'] ?? '',
        skillIds: List<String>.from(json['skill_ids'] ?? []),
        claudeMd: json['claude_md'],
        createdAt: json['created_at'] ?? '',
        updatedAt: json['updated_at'] ?? '',
      );

  Map<String, dynamic> toCreateJson() => {
        'name': name,
        if (description.isNotEmpty) 'description': description,
        if (path.isNotEmpty) 'path': path,
        if (skillIds.isNotEmpty) 'skill_ids': skillIds,
        if (claudeMd != null) 'claude_md': claudeMd,
      };
}
