import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../main.dart';

/// Returns just the summary string for a tool call: "file.txt", "ls -la",
/// "pattern", etc. Used by the activity timeline rows ("> Read file.txt")
/// and by the job_detail_screen.
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
/// text. Mirrors the approach the Jobs detail screen uses for its activity
/// panel: poll `/api/v1/jobs/{id}/logs` and parse the raw stream-json log
/// lines client-side. This is reliable because it reads from the worker's
/// persisted log (which is now populated for chat-message jobs too) instead
/// of trying to accumulate per-message state from SSE pub/sub events that
/// are vulnerable to disconnects, ordering races, and matching bugs.
///
/// While [isStreaming] is true, polls every 1.5 seconds for new entries.
/// Once streaming finishes, it fetches one final time and stops polling.
/// Completed messages mount, fetch once, and never poll again.
class ActivityTimeline extends ConsumerStatefulWidget {
  final String? jobId;
  final bool isStreaming;

  const ActivityTimeline({
    super.key,
    required this.jobId,
    required this.isStreaming,
  });

  @override
  ConsumerState<ActivityTimeline> createState() => _ActivityTimelineState();
}

class _ActivityTimelineState extends ConsumerState<ActivityTimeline> {
  List<String> _logLines = [];
  Timer? _pollTimer;
  bool _expanded = true;
  bool _hasFetchedOnce = false;

  @override
  void initState() {
    super.initState();
    if (widget.jobId != null) {
      _fetchOnce();
      if (widget.isStreaming) {
        _startPolling();
      }
    }
  }

  @override
  void didUpdateWidget(ActivityTimeline old) {
    super.didUpdateWidget(old);
    // jobId became available (was null, now set) → fetch + maybe poll
    if (old.jobId != widget.jobId && widget.jobId != null) {
      _hasFetchedOnce = false;
      _fetchOnce();
      if (widget.isStreaming) _startPolling();
    }
    // Streaming just stopped → final fetch + stop polling
    if (old.isStreaming && !widget.isStreaming) {
      _pollTimer?.cancel();
      _pollTimer = null;
      _fetchOnce();
    }
    // Streaming just started → start polling
    if (!old.isStreaming && widget.isStreaming && _pollTimer == null) {
      _startPolling();
    }
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }

  void _startPolling() {
    _pollTimer?.cancel();
    _pollTimer = Timer.periodic(
      const Duration(milliseconds: 1500),
      (_) => _fetchOnce(),
    );
  }

  Future<void> _fetchOnce() async {
    final jobId = widget.jobId;
    if (jobId == null) return;
    try {
      final logs = await ref.read(apiClientProvider).getLogs(jobId);
      if (!mounted) return;
      // Only setState if we got new content (avoid rebuild churn).
      if (logs.length != _logLines.length || !_hasFetchedOnce) {
        setState(() {
          _logLines = logs;
          _hasFetchedOnce = true;
        });
      }
    } catch (_) {
      // Polling errors are silent — next tick will retry. The first fetch
      // for a freshly-submitted job may 404 briefly while the worker is
      // still claiming it; that's expected.
    }
  }

  @override
  Widget build(BuildContext context) {
    final entries = _parseEntries();
    if (entries.isEmpty) return const SizedBox.shrink();

    final stepCount = entries.where((e) => e.kind == _EntryKind.toolUse).length;

    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: const Color(0xFF1E1E2E),
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
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              child: Row(
                children: [
                  Icon(Icons.bolt, size: 14, color: Colors.grey.shade400),
                  const SizedBox(width: 6),
                  Text(
                    'Activity ($stepCount steps)',
                    style: TextStyle(
                      color: Colors.grey.shade400,
                      fontSize: 11,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  if (widget.isStreaming) ...[
                    const SizedBox(width: 8),
                    SizedBox(
                      width: 10,
                      height: 10,
                      child: CircularProgressIndicator(
                        strokeWidth: 1.5,
                        color: Colors.grey.shade500,
                      ),
                    ),
                  ],
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
              padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final e in entries) _buildEntry(e),
                  if (widget.jobId != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 6),
                      child: TextButton.icon(
                        icon: const Icon(Icons.open_in_new, size: 12),
                        label: const Text(
                          'Open full job',
                          style: TextStyle(fontSize: 11),
                        ),
                        style: TextButton.styleFrom(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 8, vertical: 2),
                          minimumSize: const Size(0, 0),
                          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                          visualDensity: VisualDensity.compact,
                        ),
                        onPressed: () => context.go('/jobs/${widget.jobId}'),
                      ),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildEntry(_LogEntry e) {
    switch (e.kind) {
      case _EntryKind.text:
        return Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: SelectableText(
            e.text,
            style: const TextStyle(
              color: Colors.white,
              fontSize: 12,
              height: 1.4,
            ),
          ),
        );
      case _EntryKind.thinking:
        return Padding(
          padding: const EdgeInsets.only(bottom: 4),
          child: SelectableText(
            e.text.length > 300
                ? '${e.text.substring(0, 300)}...'
                : e.text,
            style: TextStyle(
              color: Colors.grey.shade600,
              fontStyle: FontStyle.italic,
              fontSize: 11,
              height: 1.3,
            ),
          ),
        );
      case _EntryKind.toolUse:
        return Padding(
          padding: const EdgeInsets.only(bottom: 4),
          child: SelectableText(
            '> ${e.toolName} ${e.text}',
            style: TextStyle(
              color: Colors.blue.shade300,
              fontFamily: 'monospace',
              fontSize: 12,
            ),
          ),
        );
      case _EntryKind.toolResult:
        return Padding(
          padding: const EdgeInsets.only(bottom: 4, left: 12),
          child: SelectableText(
            e.text,
            style: TextStyle(
              color: e.isError ? Colors.red.shade300 : Colors.grey.shade400,
              fontFamily: 'monospace',
              fontSize: 11,
              height: 1.3,
            ),
          ),
        );
      case _EntryKind.result:
        return Padding(
          padding: const EdgeInsets.only(top: 6),
          child: SelectableText(
            e.text,
            style: TextStyle(
              color: Colors.green.shade400,
              fontFamily: 'monospace',
              fontSize: 11,
            ),
          ),
        );
    }
  }

  /// Parse the raw stream-json log lines into typed activity entries.
  /// Mirrors job_detail_screen.dart's `_parseLogEntries` (which renders to
  /// widgets directly); we return structured data so the bubble can style
  /// the entries differently for inline display.
  List<_LogEntry> _parseEntries() {
    final entries = <_LogEntry>[];
    for (final raw in _logLines) {
      try {
        final val = json.decode(raw);
        if (val is! Map) continue;
        final type = val['type'] as String?;

        if (type == 'assistant') {
          final content = val['message']?['content'] as List<dynamic>? ?? [];
          for (final item in content) {
            if (item is! Map) continue;
            final ctype = item['type'] as String?;
            if (ctype == 'text') {
              final text = item['text'] as String? ?? '';
              if (text.isNotEmpty) {
                entries.add(_LogEntry(kind: _EntryKind.text, text: text));
              }
            } else if (ctype == 'tool_use') {
              final name = item['name'] as String? ?? '?';
              final input = (item['input'] as Map?)?.cast<String, dynamic>() ?? {};
              entries.add(_LogEntry(
                kind: _EntryKind.toolUse,
                toolName: name,
                text: toolSummaryShort(name, input),
              ));
            } else if (ctype == 'thinking') {
              final text = item['thinking'] as String? ?? '';
              if (text.isNotEmpty) {
                entries.add(_LogEntry(kind: _EntryKind.thinking, text: text));
              }
            }
          }
        } else if (type == 'user') {
          final content = val['message']?['content'] as List<dynamic>? ?? [];
          for (final item in content) {
            if (item is! Map) continue;
            if (item['type'] != 'tool_result') continue;
            final isError = item['is_error'] == true;
            final text = _extractToolResultText(item['content']);
            if (text.isEmpty) continue;
            final truncated = text.length > 500 ? '${text.substring(0, 500)}...' : text;
            entries.add(_LogEntry(
              kind: _EntryKind.toolResult,
              text: truncated,
              isError: isError,
            ));
          }
        } else if (type == 'result') {
          final result = val['result'] as String? ?? '';
          final cost = val['total_cost_usd'] as num? ?? 0;
          final dur = val['duration_ms'] as num? ?? 0;
          if (result.isNotEmpty || cost > 0) {
            entries.add(_LogEntry(
              kind: _EntryKind.result,
              text: '— Result (cost: \$${cost.toStringAsFixed(4)}, '
                  '${(dur / 1000).toStringAsFixed(1)}s)',
            ));
          }
        }
        // Skip 'system' type
      } catch (_) {
        // Non-JSON line — silently skip (the worker may emit non-JSON
        // diagnostics during startup/teardown).
      }
    }
    return entries;
  }

  String _extractToolResultText(dynamic content) {
    if (content is String) return content;
    if (content is List) {
      return content
          .whereType<Map>()
          .map((m) => m['text'])
          .whereType<String>()
          .join('\n');
    }
    return '';
  }
}

enum _EntryKind { text, thinking, toolUse, toolResult, result }

class _LogEntry {
  final _EntryKind kind;
  final String text;
  final String toolName;
  final bool isError;

  _LogEntry({
    required this.kind,
    this.text = '',
    this.toolName = '',
    this.isError = false,
  });
}

