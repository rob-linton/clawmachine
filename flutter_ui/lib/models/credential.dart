class Credential {
  final String id;
  final String name;
  final String description;
  final List<String> keys;
  final Map<String, String> maskedValues;

  Credential({
    required this.id,
    required this.name,
    this.description = '',
    this.keys = const [],
    this.maskedValues = const {},
  });

  factory Credential.fromJson(Map<String, dynamic> json) => Credential(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        description: json['description'] ?? '',
        keys: List<String>.from(json['keys'] ?? []),
        maskedValues: (json['masked_values'] as Map<String, dynamic>?)
                ?.map((k, v) => MapEntry(k, v.toString())) ??
            {},
      );
}
