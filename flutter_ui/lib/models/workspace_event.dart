class WorkspaceEvent {
  final String timestamp;
  final String eventType;
  final String? relatedId;
  final String description;

  WorkspaceEvent({
    required this.timestamp,
    required this.eventType,
    this.relatedId,
    required this.description,
  });

  factory WorkspaceEvent.fromJson(Map<String, dynamic> json) => WorkspaceEvent(
        timestamp: json['timestamp'] ?? '',
        eventType: json['event_type'] ?? '',
        relatedId: json['related_id'],
        description: json['description'] ?? '',
      );
}
