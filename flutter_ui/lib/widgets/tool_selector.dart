import 'package:flutter/material.dart';
import '../models/tool.dart';

/// Compact tool selector that shows selected count and opens a dialog to pick tools.
class ToolSelector extends StatelessWidget {
  final List<Tool> availableTools;
  final Set<String> selectedIds;
  final ValueChanged<Set<String>> onChanged;
  final String label;

  const ToolSelector({
    super.key,
    required this.availableTools,
    required this.selectedIds,
    required this.onChanged,
    this.label = 'CLI Tools',
  });

  @override
  Widget build(BuildContext context) {
    final selectedNames = availableTools
        .where((t) => selectedIds.contains(t.id))
        .map((t) => t.name)
        .toList();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Semantics(
              label: label,
              child: Text(label,
                  style: const TextStyle(
                      fontWeight: FontWeight.bold, fontSize: 13)),
            ),
            const Spacer(),
            OutlinedButton.icon(
              onPressed: () async {
                final result = await _showSelectionDialog(context);
                if (result != null) {
                  onChanged(result);
                }
              },
              icon: const Icon(Icons.build_circle, size: 16),
              label: Semantics(
                label: selectedIds.isEmpty
                    ? 'Select CLI Tools'
                    : '${selectedIds.length} tools selected',
                child: Text(
                  selectedIds.isEmpty
                      ? 'Select Tools'
                      : '${selectedIds.length} selected',
                ),
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
                      label: Semantics(
                        label: 'Tool $name',
                        child: Text(name,
                            style: const TextStyle(fontSize: 12)),
                      ),
                      visualDensity: VisualDensity.compact,
                      deleteIcon: const Icon(Icons.close, size: 14),
                      onDeleted: () {
                        final updated = Set<String>.from(selectedIds);
                        final tool =
                            availableTools.firstWhere((t) => t.name == name);
                        updated.remove(tool.id);
                        onChanged(updated);
                      },
                    ))
                .toList(),
          ),
        ],
        if (availableTools.isEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Semantics(
              label: 'No CLI tools available',
              child: Text('No CLI tools available. Add tools first.',
                  style: TextStyle(color: Colors.grey[500], fontSize: 12)),
            ),
          ),
      ],
    );
  }

  Future<Set<String>?> _showSelectionDialog(BuildContext context) async {
    final selected = Set<String>.from(selectedIds);
    final searchCtrl = TextEditingController();
    var filtered = List<Tool>.from(availableTools);

    return showDialog<Set<String>>(
      barrierDismissible: false,
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialogState) {
          void updateFilter() {
            final query = searchCtrl.text.toLowerCase();
            filtered = query.isEmpty
                ? List<Tool>.from(availableTools)
                : availableTools
                    .where((t) =>
                        t.name.toLowerCase().contains(query) ||
                        t.id.toLowerCase().contains(query) ||
                        t.description.toLowerCase().contains(query) ||
                        t.tags.any((tag) => tag.toLowerCase().contains(query)))
                    .toList();
          }

          return AlertDialog(
            title: Row(
              children: [
                Semantics(
                  header: true,
                  label: 'Select CLI Tools',
                  child: const Text('Select CLI Tools'),
                ),
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
                  TextField(
                    controller: searchCtrl,
                    decoration: const InputDecoration(
                      hintText: 'Search tools by name, ID, or tag...',
                      prefixIcon: Icon(Icons.search),
                      isDense: true,
                      border: OutlineInputBorder(),
                    ),
                    onChanged: (_) => setDialogState(() => updateFilter()),
                  ),
                  const SizedBox(height: 12),
                  Expanded(
                    child: filtered.isEmpty
                        ? const Center(
                            child: Text('No tools match your search.'))
                        : ListView.builder(
                            itemCount: filtered.length,
                            itemBuilder: (context, i) {
                              final tool = filtered[i];
                              final isSelected = selected.contains(tool.id);
                              return CheckboxListTile(
                                dense: true,
                                value: isSelected,
                                title: Semantics(
                                  label: 'Tool ${tool.name}',
                                  child: Text(tool.name),
                                ),
                                subtitle: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    if (tool.description.isNotEmpty)
                                      Text(tool.description,
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: const TextStyle(fontSize: 12)),
                                    Row(
                                      children: [
                                        Text(tool.id,
                                            style: TextStyle(
                                                fontSize: 11,
                                                fontFamily: 'monospace',
                                                color: Colors.grey[500])),
                                        if (tool.checkCommand.isNotEmpty) ...[
                                          const SizedBox(width: 8),
                                          Text(tool.checkCommand,
                                              style: TextStyle(
                                                  fontSize: 11,
                                                  fontFamily: 'monospace',
                                                  color: Colors.grey[500])),
                                        ],
                                        if (tool.tags.isNotEmpty) ...[
                                          const SizedBox(width: 8),
                                          Text(tool.tags.join(', '),
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
                                      selected.add(tool.id);
                                    } else {
                                      selected.remove(tool.id);
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
