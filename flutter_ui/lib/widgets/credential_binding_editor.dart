import 'package:flutter/material.dart';
import '../models/tool.dart';
import '../models/credential.dart';

/// Shows a dropdown for each selected tool (that has envVars) to bind a credential to it.
class CredentialBindingEditor extends StatelessWidget {
  final Set<String> selectedToolIds;
  final List<Tool> availableTools;
  final List<Credential> availableCredentials;
  final Map<String, String> bindings; // tool_id -> credential_id
  final ValueChanged<Map<String, String>> onChanged;
  final String label;

  const CredentialBindingEditor({
    super.key,
    required this.selectedToolIds,
    required this.availableTools,
    required this.availableCredentials,
    required this.bindings,
    required this.onChanged,
    this.label = 'Credential Bindings',
  });

  @override
  Widget build(BuildContext context) {
    // Filter to tools that are selected AND have non-empty envVars
    final matchingTools = availableTools
        .where((t) => selectedToolIds.contains(t.id) && t.envVars.isNotEmpty)
        .toList();

    if (matchingTools.isEmpty) {
      return const SizedBox.shrink();
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            const Icon(Icons.key, size: 16),
            const SizedBox(width: 6),
            Semantics(
              label: label,
              child: Text(
                label,
                style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 13),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        ...matchingTools.map((tool) {
          final envKeys = tool.envVars.map((e) => e.key).join(', ');
          final currentValue = bindings[tool.id];

          return Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Row(
              children: [
                Expanded(
                  flex: 2,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Semantics(
                        label: 'Tool ${tool.name}',
                        child: Text(
                          tool.name,
                          style: const TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 13,
                          ),
                        ),
                      ),
                      Semantics(
                        label: 'Needs $envKeys',
                        child: Text(
                          'Needs: $envKeys',
                          style: TextStyle(
                            fontSize: 11,
                            color: Colors.grey[500],
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  flex: 3,
                  child: Semantics(
                    label: 'Credential for ${tool.name}: ${_credentialNameFor(currentValue) ?? "None"}',
                    child: DropdownButton<String?>(
                      value: currentValue,
                      isExpanded: true,
                      hint: const Text('None'),
                      items: [
                        const DropdownMenuItem<String?>(
                          value: null,
                          child: Text('None'),
                        ),
                        ...availableCredentials.map(
                          (cred) => DropdownMenuItem<String?>(
                            value: cred.id,
                            child: Text(cred.name),
                          ),
                        ),
                      ],
                      onChanged: (value) {
                        final updated = Map<String, String>.from(bindings);
                        if (value == null) {
                          updated.remove(tool.id);
                        } else {
                          updated[tool.id] = value;
                        }
                        onChanged(updated);
                      },
                    ),
                  ),
                ),
              ],
            ),
          );
        }),
      ],
    );
  }

  String? _credentialNameFor(String? credentialId) {
    if (credentialId == null) return null;
    final match = availableCredentials.where((c) => c.id == credentialId);
    return match.isNotEmpty ? match.first.name : null;
  }
}
