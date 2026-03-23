class Skill {
  final String id;
  final String name;
  final String content;
  final String description;
  final List<String> tags;
  final Map<String, String> files;
  final String version;
  final String author;
  final String? license;
  final String? sourceUrl;
  final bool enabled;
  final String? createdAt;
  final String? updatedAt;

  Skill({
    required this.id,
    required this.name,
    required this.content,
    this.description = '',
    this.tags = const [],
    this.files = const {},
    this.version = '',
    this.author = '',
    this.license,
    this.sourceUrl,
    this.enabled = true,
    this.createdAt,
    this.updatedAt,
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
        version: json['version'] ?? '',
        author: json['author'] ?? '',
        license: json['license'],
        sourceUrl: json['source_url'],
        enabled: json['enabled'] ?? true,
        createdAt: json['created_at'],
        updatedAt: json['updated_at'],
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'content': content,
        'description': description,
        'tags': tags,
        'files': files,
        'version': version,
        'author': author,
        if (license != null) 'license': license,
        if (sourceUrl != null) 'source_url': sourceUrl,
        'enabled': enabled,
      };
}
