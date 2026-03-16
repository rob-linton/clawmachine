class Skill {
  final String id;
  final String name;
  final String content;
  final String description;
  final List<String> tags;
  final Map<String, String> files;

  Skill({
    required this.id,
    required this.name,
    required this.content,
    this.description = '',
    this.tags = const [],
    this.files = const {},
  });

  factory Skill.fromJson(Map<String, dynamic> json) => Skill(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        content: json['content'] ?? '',
        description: json['description'] ?? '',
        tags: List<String>.from(json['tags'] ?? []),
        files: (json['files'] as Map<String, dynamic>?)
                ?.map((k, v) => MapEntry(k, v.toString())) ??
            {},
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'content': content,
        'description': description,
        'tags': tags,
        'files': files,
      };
}
