import 'package:flutter/material.dart';
import '../models/skill.dart';

/// Compact skill selector that shows selected count and opens a dialog to pick skills.
class SkillSelector extends StatelessWidget {
  final List<Skill> availableSkills;
  final Set<String> selectedIds;
  final ValueChanged<Set<String>> onChanged;
  final String label;

  const SkillSelector({
    super.key,
    required this.availableSkills,
    required this.selectedIds,
    required this.onChanged,
    this.label = 'Skills',
  });

  @override
  Widget build(BuildContext context) {
    final selectedNames = availableSkills
        .where((s) => selectedIds.contains(s.id))
        .map((s) => s.name)
        .toList();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(label, style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 13)),
            const Spacer(),
            OutlinedButton.icon(
              onPressed: () async {
                final result = await _showSelectionDialog(context);
                if (result != null) {
                  onChanged(result);
                }
              },
              icon: const Icon(Icons.checklist, size: 16),
              label: Text(
                selectedIds.isEmpty
                    ? 'Select Skills'
                    : '${selectedIds.length} selected',
              ),
            ),
          ],
        ),
        if (selectedNames.isNotEmpty) ...[
          const SizedBox(height: 6),
          Wrap(
            spacing: 6,
            runSpacing: 4,
            children: selectedNames
                .map((name) => Chip(
                      label: Text(name, style: const TextStyle(fontSize: 12)),
                      visualDensity: VisualDensity.compact,
                      deleteIcon: const Icon(Icons.close, size: 14),
                      onDeleted: () {
                        final updated = Set<String>.from(selectedIds);
                        final skill = availableSkills.firstWhere((s) => s.name == name);
                        updated.remove(skill.id);
                        onChanged(updated);
                      },
                    ))
                .toList(),
          ),
        ],
        if (availableSkills.isEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Text('No skills available. Import skills first.',
                style: TextStyle(color: Colors.grey[500], fontSize: 12)),
          ),
      ],
    );
  }

  Future<Set<String>?> _showSelectionDialog(BuildContext context) async {
    final selected = Set<String>.from(selectedIds);
    final searchCtrl = TextEditingController();
    var filtered = List<Skill>.from(availableSkills);

    return showDialog<Set<String>>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) {
          void updateFilter() {
            final query = searchCtrl.text.toLowerCase();
            filtered = query.isEmpty
                ? List<Skill>.from(availableSkills)
                : availableSkills
                    .where((s) =>
                        s.name.toLowerCase().contains(query) ||
                        s.id.toLowerCase().contains(query) ||
                        s.description.toLowerCase().contains(query) ||
                        s.tags.any((t) => t.toLowerCase().contains(query)))
                    .toList();
          }

          return AlertDialog(
            title: Row(
              children: [
                const Text('Select Skills'),
                const Spacer(),
                Text('${selected.length} selected',
                    style: TextStyle(fontSize: 14, color: Colors.grey[400])),
              ],
            ),
            content: SizedBox(
              width: 500,
              height: 400,
              child: Column(
                children: [
                  // Search bar
                  TextField(
                    controller: searchCtrl,
                    decoration: const InputDecoration(
                      hintText: 'Search skills by name, ID, or tag...',
                      prefixIcon: Icon(Icons.search),
                      isDense: true,
                      border: OutlineInputBorder(),
                    ),
                    onChanged: (_) => setDialogState(() => updateFilter()),
                  ),
                  const SizedBox(height: 12),

                  // Skill list
                  Expanded(
                    child: filtered.isEmpty
                        ? const Center(child: Text('No skills match your search.'))
                        : ListView.builder(
                            itemCount: filtered.length,
                            itemBuilder: (context, i) {
                              final skill = filtered[i];
                              final isSelected = selected.contains(skill.id);
                              return CheckboxListTile(
                                dense: true,
                                value: isSelected,
                                title: Text(skill.name),
                                subtitle: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    if (skill.description.isNotEmpty)
                                      Text(skill.description,
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: const TextStyle(fontSize: 12)),
                                    Row(
                                      children: [
                                        Text(skill.id,
                                            style: TextStyle(
                                                fontSize: 11,
                                                fontFamily: 'monospace',
                                                color: Colors.grey[500])),
                                        if (skill.files.isNotEmpty) ...[
                                          const SizedBox(width: 8),
                                          Text('${skill.files.length} files',
                                              style: TextStyle(
                                                  fontSize: 11,
                                                  color: Colors.grey[500])),
                                        ],
                                        if (skill.tags.isNotEmpty) ...[
                                          const SizedBox(width: 8),
                                          Text(skill.tags.join(', '),
                                              style: TextStyle(
                                                  fontSize: 11,
                                                  color: Colors.grey[500])),
                                        ],
                                      ],
                                    ),
                                  ],
                                ),
                                onChanged: (v) {
                                  setDialogState(() {
                                    if (v == true) {
                                      selected.add(skill.id);
                                    } else {
                                      selected.remove(skill.id);
                                    }
                                  });
                                },
                              );
                            },
                          ),
                  ),
                ],
              ),
            ),
            actions: [
              if (selected.isNotEmpty)
                TextButton(
                  onPressed: () => setDialogState(() => selected.clear()),
                  child: const Text('Clear All'),
                ),
              TextButton(
                onPressed: () => Navigator.pop(ctx),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(ctx, selected),
                child: Text('Done (${selected.length})'),
              ),
            ],
          );
        },
      ),
    );
  }
}
