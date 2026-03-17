class Workspace {
  final String id;
  final String name;
  final String description;
  final String? path; // legacy only
  final List<String> skillIds;
  final String? claudeMd;
  final String persistence; // "ephemeral", "persistent", "snapshot"
  final String? remoteUrl;
  final String? baseImage;
  final String createdAt;
  final String updatedAt;

  Workspace({
    required this.id,
    required this.name,
    this.description = '',
    this.path,
    this.skillIds = const [],
    this.claudeMd,
    this.persistence = 'persistent',
    this.remoteUrl,
    this.baseImage,
    required this.createdAt,
    required this.updatedAt,
  });

  bool get isLegacy => path != null && path!.isNotEmpty;

  factory Workspace.fromJson(Map<String, dynamic> json) => Workspace(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        description: json['description'] ?? '',
        path: json['path'],
        skillIds: List<String>.from(json['skill_ids'] ?? []),
        claudeMd: json['claude_md'],
        persistence: json['persistence'] ?? 'persistent',
        remoteUrl: json['remote_url'],
        baseImage: json['base_image'],
        createdAt: json['created_at'] ?? '',
        updatedAt: json['updated_at'] ?? '',
      );

  Map<String, dynamic> toCreateJson() => {
        'name': name,
        if (description.isNotEmpty) 'description': description,
        if (path != null && path!.isNotEmpty) 'path': path,
        if (skillIds.isNotEmpty) 'skill_ids': skillIds,
        if (claudeMd != null) 'claude_md': claudeMd,
        if (persistence != 'persistent') 'persistence': persistence,
        if (remoteUrl != null && remoteUrl!.isNotEmpty) 'remote_url': remoteUrl,
        if (baseImage != null && baseImage!.isNotEmpty) 'base_image': baseImage,
      };
}
