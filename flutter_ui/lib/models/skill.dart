class Skill {
  final String id;
  final String name;
  final String skillType;
  final String content;
  final String description;
  final List<String> tags;

  Skill({
    required this.id,
    required this.name,
    required this.skillType,
    required this.content,
    this.description = '',
    this.tags = const [],
  });

  factory Skill.fromJson(Map<String, dynamic> json) => Skill(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        skillType: json['skill_type'] ?? 'template',
        content: json['content'] ?? '',
        description: json['description'] ?? '',
        tags: List<String>.from(json['tags'] ?? []),
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'skill_type': skillType,
        'content': content,
        'description': description,
        'tags': tags,
      };
}
