import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../main.dart';

/// Returns just the summary string for a tool call: "file.txt", "ls -la",
/// "pattern", etc. Used by the activity timeline rows ("> Read file.txt").
/// Mirrors the helper that used to live in job_detail_screen.dart.
String toolSummaryShort(String name, Map<String, dynamic> input) {
  switch (name) {
    case 'Read':
    case 'Write':
    case 'Edit':
      return input['file_path']?.toString() ?? '';
    case 'Bash':
      final cmd = input['command']?.toString() ?? '';
      return cmd.length > 80 ? '${cmd.substring(0, 80)}...' : cmd;
    case 'Glob':
      return input['pattern']?.toString() ?? '';
    case 'Grep':
      return input['pattern']?.toString() ?? '';
    case 'WebFetch':
      return input['url']?.toString() ?? '';
    case 'WebSearch':
      return input['query']?.toString() ?? '';
    default:
      return '';
  }
}

/// Returns a humanized verb form: "Reading file.txt", "Running ls -la".
/// Used by the legacy in-flight `_toolStatus` caption in the chat
/// controller. Kept for backward compatibility.
String toolSummaryHumanized(String tool, String summary) {
  switch (tool) {
    case 'Read':
      return 'Reading ${summary.isNotEmpty ? summary : "file"}';
    case 'Write':
      return 'Writing ${summary.isNotEmpty ? summary : "file"}';
    case 'Edit':
      return 'Editing ${summary.isNotEmpty ? summary : "file"}';
    case 'Bash':
      return 'Running ${summary.isNotEmpty ? summary : "command"}';
    case 'Grep':
      return 'Searching ${summary.isNotEmpty ? summary : "files"}';
    case 'Glob':
      return 'Finding files';
    default:
      return summary.isNotEmpty ? '$tool: $summary' : tool;
  }
}

/// Inline activity timeline rendered above an assistant message's response
/// text. Shows each tool_use the assistant made and (when available) the
/// tool_result it received back, in chronological order.
///
/// Defaults to expanded while [isStreaming] is true; once streaming finishes
/// it remains expanded for a smooth handover, then the user can collapse
/// via the chevron in the header.
class ActivityTimeline extends ConsumerStatefulWidget {
  final List<Map<String, dynamic>> entries;
  final bool isStreaming;
  final String? jobId;

  const ActivityTimeline({
    super.key,
    required this.entries,
    required this.isStreaming,
    this.jobId,
  });

  @override
  ConsumerState<ActivityTimeline> createState() => _ActivityTimelineState();
}

class _ActivityTimelineState extends ConsumerState<ActivityTimeline> {
  bool _expanded = true;
  // Cache of fetched job log lines so the "Show full" button doesn't
  // re-hit /jobs/{id}/logs on every click.
  List<String>? _cachedLogLines;

  @override
  void didUpdateWidget(ActivityTimeline old) {
    super.didUpdateWidget(old);
    // When streaming finishes, leave the timeline expanded for a smooth
    // handover. The user can manually collapse via the chevron.
    if (old.isStreaming && !widget.isStreaming) {
      // No-op: already expanded by default. Listed for parallel symmetry
      // with the thinking-section pattern.
    }
  }

  @override
  Widget build(BuildContext context) {
    final entries = widget.entries;
    if (entries.isEmpty) return const SizedBox.shrink();

    // Pair tool_use entries with their tool_result by tool_use_id.
    final completedIds = entries
        .where((e) => e['type'] == 'tool_result')
        .map((e) => e['tool_use_id'])
        .toSet();
    final resultsByToolUseId = <String, Map<String, dynamic>>{};
    for (final e in entries) {
      if (e['type'] == 'tool_result') {
        final id = e['tool_use_id'] as String?;
        if (id != null && id.isNotEmpty) resultsByToolUseId[id] = e;
      }
    }

    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: Colors.grey.shade900.withValues(alpha: 0.3),
        borderRadius: BorderRadius.circular(6),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Header — always visible, click to toggle expanded state.
          InkWell(
            onTap: () => setState(() => _expanded = !_expanded),
            borderRadius: BorderRadius.circular(6),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
              child: Row(
                children: [
                  Icon(Icons.bolt, size: 14, color: Colors.grey.shade400),
                  const SizedBox(width: 4),
                  Text(
                    'Activity (${entries.where((e) => e['type'] == 'tool_use').length} steps)',
                    style: TextStyle(
                      color: Colors.grey.shade400,
                      fontSize: 11,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  const Spacer(),
                  Icon(
                    _expanded ? Icons.expand_less : Icons.expand_more,
                    size: 16,
                    color: Colors.grey.shade500,
                  ),
                ],
              ),
            ),
          ),
          if (_expanded)
            Padding(
              padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final entry in entries)
                    if (entry['type'] == 'tool_use')
                      _buildToolUseRow(entry, completedIds, resultsByToolUseId)
                    else if (entry['type'] == 'tool_result')
                      _buildToolResultRow(entry),
                ],
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildToolUseRow(
    Map<String, dynamic> entry,
    Set<dynamic> completedIds,
    Map<String, Map<String, dynamic>> resultsByToolUseId,
  ) {
    final tool = entry['tool'] as String? ?? '?';
    final summary = entry['summary'] as String? ?? '';
    final toolUseId = entry['tool_use_id'] as String? ?? '';
    final isCancelled = entry['cancelled'] == true;
    final result = resultsByToolUseId[toolUseId];
    final hasResult = result != null;
    final isError = hasResult && result['is_error'] == true;
    final isInflight = !hasResult && !isCancelled;

    Widget statusIcon;
    if (isCancelled) {
      statusIcon = Icon(Icons.block, size: 12, color: Colors.grey.shade600);
    } else if (isInflight) {
      statusIcon = widget.isStreaming
          ? SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                color: Colors.grey.shade500,
              ),
            )
          : Icon(Icons.help_outline, size: 12, color: Colors.grey.shade600);
    } else if (isError) {
      statusIcon = const Icon(Icons.close, size: 12, color: Colors.redAccent);
    } else {
      statusIcon = Icon(Icons.check, size: 12, color: Colors.green.shade400);
    }

    final textStyle = TextStyle(
      color: isCancelled ? Colors.grey.shade600 : Colors.blue.shade300,
      fontFamily: 'monospace',
      fontSize: 12,
      decoration:
          isCancelled ? TextDecoration.lineThrough : TextDecoration.none,
    );

    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(width: 14, child: Center(child: statusIcon)),
          const SizedBox(width: 6),
          Expanded(
            child: SelectableText(
              '> $tool ${summary.isNotEmpty ? summary : ""}',
              style: textStyle,
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildToolResultRow(Map<String, dynamic> entry) {
    final output = entry['output'] as String? ?? '';
    final truncated = entry['truncated'] == true;
    final isError = entry['is_error'] == true;
    if (output.isEmpty && !truncated) return const SizedBox.shrink();

    final color = isError ? Colors.red.shade300 : Colors.grey.shade400;

    return Padding(
      padding: const EdgeInsets.only(left: 20, bottom: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SelectableText(
            output,
            style: TextStyle(
              color: color,
              fontFamily: 'monospace',
              fontSize: 11,
              height: 1.3,
            ),
          ),
          if (truncated)
            TextButton.icon(
              icon: const Icon(Icons.unfold_more, size: 12),
              label: const Text('Show full', style: TextStyle(fontSize: 11)),
              style: TextButton.styleFrom(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                minimumSize: const Size(0, 0),
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                visualDensity: VisualDensity.compact,
              ),
              onPressed: widget.jobId == null
                  ? null
                  : () => _showFullOutput(entry),
            ),
        ],
      ),
    );
  }

  Future<void> _showFullOutput(Map<String, dynamic> entry) async {
    final jobId = widget.jobId;
    if (jobId == null) return;
    final toolUseId = entry['tool_use_id'] as String? ?? '';
    final fallbackOutput = entry['output'] as String? ?? '';

    try {
      _cachedLogLines ??= await ref.read(apiClientProvider).getLogs(jobId);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to load full output: $e')),
        );
      }
      return;
    }

    final fullText = _findToolResultInLogs(_cachedLogLines!, toolUseId) ??
        '$fallbackOutput\n\n[Could not find full output in job logs — showing truncated view]';
    if (!mounted) return;
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Row(
          children: [
            const Expanded(child: Text('Tool output', style: TextStyle(fontSize: 14))),
            IconButton(
              icon: const Icon(Icons.copy, size: 18),
              tooltip: 'Copy',
              onPressed: () {
                Clipboard.setData(ClipboardData(text: fullText));
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(
                    content: Text('Copied'),
                    duration: Duration(seconds: 1),
                  ),
                );
              },
            ),
            IconButton(
              icon: const Icon(Icons.open_in_new, size: 18),
              tooltip: 'Open in job',
              onPressed: () {
                Navigator.pop(ctx);
                context.go('/jobs/$jobId');
              },
            ),
          ],
        ),
        content: SizedBox(
          width: 800,
          height: 600,
          child: SingleChildScrollView(
            child: SelectableText(
              fullText,
              style: const TextStyle(
                fontFamily: 'monospace',
                fontSize: 12,
                height: 1.4,
              ),
            ),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
        ],
      ),
    );
  }

  /// Walk the raw stream-json log lines, find a `user` message containing a
  /// `tool_result` block whose `tool_use_id` matches, and extract the text
  /// content. Mirrors the Rust `extract_tool_result_text` helper.
  String? _findToolResultInLogs(List<String> lines, String toolUseId) {
    for (final raw in lines) {
      try {
        final val = json.decode(raw);
        if (val is! Map) continue;
        if (val['type'] != 'user') continue;
        final content = val['message']?['content'] as List?;
        if (content == null) continue;
        for (final item in content) {
          if (item is! Map) continue;
          if (item['type'] != 'tool_result') continue;
          if (item['tool_use_id'] != toolUseId) continue;
          final c = item['content'];
          if (c is String) return c;
          if (c is List) {
            return c
                .whereType<Map>()
                .map((m) => m['text'])
                .whereType<String>()
                .join('\n');
          }
        }
      } catch (_) {}
    }
    return null;
  }
}
