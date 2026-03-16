import 'package:flutter/material.dart';
import '../models/skill.dart';

/// Reusable skill selector widget.
/// Shows available skills as FilterChips with multi-select.
/// Can also open a full dialog for browsing skills with descriptions.
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
    if (availableSkills.isEmpty) {
      return Text('No skills available. Import skills first.',
          style: TextStyle(color: Colors.grey[500], fontSize: 13));
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(label, style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 13)),
            const Spacer(),
            if (selectedIds.isNotEmpty)
              TextButton(
                onPressed: () => onChanged({}),
                child: Text('Clear (${selectedIds.length})', style: const TextStyle(fontSize: 12)),
              ),
          ],
        ),
        const SizedBox(height: 4),
        Wrap(
          spacing: 6,
          runSpacing: 4,
          children: availableSkills.map((skill) {
            final selected = selectedIds.contains(skill.id);
            return Semantics(
              label: 'Skill ${skill.name}',
              child: FilterChip(
                label: Text(skill.name),
                tooltip: skill.description.isNotEmpty ? skill.description : skill.id,
                selected: selected,
                onSelected: (v) {
                  final updated = Set<String>.from(selectedIds);
                  if (v) {
                    updated.add(skill.id);
                  } else {
                    updated.remove(skill.id);
                  }
                  onChanged(updated);
                },
              ),
            );
          }).toList(),
        ),
      ],
    );
  }
}

/// Shows a dialog to select skills. Returns the updated set of selected IDs.
Future<Set<String>?> showSkillSelectorDialog({
  required BuildContext context,
  required List<Skill> availableSkills,
  required Set<String> currentSelection,
  String title = 'Select Skills',
}) async {
  final selected = Set<String>.from(currentSelection);

  return showDialog<Set<String>>(
    context: context,
    builder: (ctx) => StatefulBuilder(
      builder: (ctx, setDialogState) => AlertDialog(
        title: Text(title),
        content: SizedBox(
          width: 500,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (availableSkills.isEmpty)
                  const Padding(
                    padding: EdgeInsets.all(24),
                    child: Text('No skills available. Import skills first.'),
                  )
                else
                  ...availableSkills.map((skill) => CheckboxListTile(
                        dense: true,
                        value: selected.contains(skill.id),
                        title: Text(skill.name),
                        subtitle: skill.description.isNotEmpty
                            ? Text(skill.description, maxLines: 1, overflow: TextOverflow.ellipsis)
                            : Text(skill.id, style: const TextStyle(fontSize: 11, color: Colors.grey)),
                        secondary: skill.files.isNotEmpty
                            ? Text('${skill.files.length} files',
                                style: const TextStyle(fontSize: 11, color: Colors.grey))
                            : null,
                        onChanged: (v) {
                          setDialogState(() {
                            if (v == true) {
                              selected.add(skill.id);
                            } else {
                              selected.remove(skill.id);
                            }
                          });
                        },
                      )),
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, selected),
            child: Text('Done (${selected.length} selected)'),
          ),
        ],
      ),
    ),
  );
}
