class ToolEnvVar {
  final String key;
  final String description;
  final bool required;

  ToolEnvVar({required this.key, this.description = '', this.required = true});

  factory ToolEnvVar.fromJson(Map<String, dynamic> json) => ToolEnvVar(
        key: json['key'] ?? '',
        description: json['description'] ?? '',
        required: json['required'] ?? true,
      );

  Map<String, dynamic> toJson() => {
        'key': key,
        'description': description,
        'required': required,
      };
}

class Tool {
  final String id;
  final String name;
  final String description;
  final List<String> tags;
  final String installCommands;
  final String checkCommand;
  final List<ToolEnvVar> envVars;
  final String? authScript;

  Tool({
    required this.id,
    required this.name,
    this.description = '',
    this.tags = const [],
    required this.installCommands,
    required this.checkCommand,
    this.envVars = const [],
    this.authScript,
  });

  factory Tool.fromJson(Map<String, dynamic> json) => Tool(
        id: json['id'] ?? '',
        name: json['name'] ?? '',
        description: json['description'] ?? '',
        tags: List<String>.from(json['tags'] ?? []),
        installCommands: json['install_commands'] ?? '',
        checkCommand: json['check_command'] ?? '',
        envVars: (json['env_vars'] as List?)
                ?.map((e) => ToolEnvVar.fromJson(e))
                .toList() ??
            [],
        authScript: json['auth_script'],
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'description': description,
        'tags': tags,
        'install_commands': installCommands,
        'check_command': checkCommand,
        'env_vars': envVars.map((e) => e.toJson()).toList(),
        if (authScript != null) 'auth_script': authScript,
      };
}
