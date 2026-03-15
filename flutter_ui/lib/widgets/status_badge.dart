import 'package:flutter/material.dart';

class StatusBadge extends StatelessWidget {
  final String status;
  const StatusBadge({super.key, required this.status});

  @override
  Widget build(BuildContext context) {
    final (color, icon) = switch (status) {
      'pending' => (Colors.orange, Icons.schedule),
      'running' => (Colors.blue, Icons.play_circle),
      'completed' => (Colors.green, Icons.check_circle),
      'failed' => (Colors.red, Icons.error),
      'cancelled' => (Colors.grey, Icons.cancel),
      _ => (Colors.grey, Icons.help),
    };

    return Chip(
      avatar: Icon(icon, size: 16, color: color),
      label: Text(status, style: TextStyle(color: color, fontSize: 12)),
      backgroundColor: color.withValues(alpha: 0.1),
      side: BorderSide.none,
      padding: EdgeInsets.zero,
      visualDensity: VisualDensity.compact,
    );
  }
}
