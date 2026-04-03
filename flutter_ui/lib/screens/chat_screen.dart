import 'dart:async';
import 'dart:convert';
import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:url_launcher/url_launcher.dart';
import '../main.dart';
import '../widgets/markdown_message.dart';

class ChatScreen extends ConsumerStatefulWidget {
  const ChatScreen({super.key});

  @override
  ConsumerState<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends ConsumerState<ChatScreen> {
  final _inputController = TextEditingController();
  final _inputFocusNode = FocusNode();
  final _scrollController = ScrollController();
  List<Map<String, dynamic>> _messages = [];
  Map<String, dynamic>? _session;
  bool _loading = true;
  String? _error;
  final Map<String, int> _pendingJobs = {}; // job_id → seq
  String _selectedModel = 'sonnet';
  String _searchQuery = '';
  List<Map<String, dynamic>>? _searchResults;
  List<Map<String, dynamic>> _artifacts = [];
  StreamSubscription? _eventSub;
  StreamSubscription? _streamSub;
  Timer? _pollTimer;
  final Map<int, String> _thinkingContent = {}; // seq → accumulated thinking
  String _toolStatus = ''; // current tool activity line
  bool _cancelling = false; // true between cancel press and worker confirmation

  @override
  void initState() {
    super.initState();
    _initChat();
    _connectChatStream();
    // Listen for job completion via SSE
    _eventSub = ref.read(eventServiceProvider).jobUpdates.listen((event) {
      final jobId = event['job_id'] as String?;
      if (jobId != null && _pendingJobs.containsKey(jobId)) {
        final status = event['status'] as String?;
        if (status == 'completed' || status == 'failed') {
          _onJobComplete(jobId);
        }
      }
    });
    // Polling fallback: SSE events are unreliable (Redis pub/sub is
    // fire-and-forget). Poll every 2s to clear any stuck messages.
    _pollTimer = Timer.periodic(const Duration(seconds: 2), (_) {
      final thinkingCount = _messages.where((m) => m['_thinking'] == true).length;
      final pendingCount = _pendingJobs.length;
      print('[POLL] pending=$pendingCount thinking=$thinkingCount msgs=${_messages.length}');
      if (!mounted || (_pendingJobs.isEmpty && thinkingCount == 0)) return;
      print('[POLL] refreshing...');
      _refreshMessages();
    });
  }

  @override
  void dispose() {
    _inputController.dispose();
    _inputFocusNode.dispose();
    _scrollController.dispose();
    _eventSub?.cancel();
    _streamSub?.cancel();
    _pollTimer?.cancel();
    super.dispose();
  }

  Future<void> _initChat() async {
    setState(() => _loading = true);
    try {
      final api = ref.read(apiClientProvider);
      final session = await api.createOrGetChat();
      final messages = await api.getChatMessages(limit: 100);
      final artifacts = await api.getArtifacts().catchError((_) => <Map<String, dynamic>>[]);
      setState(() {
        _session = session;
        _messages = messages;
        _artifacts = artifacts;
        _loading = false;
      });
      _scrollToBottom();
    } catch (e) {
      setState(() {
        _error = 'Failed to load chat: $e';
        _loading = false;
      });
    }
  }

  Future<void> _refreshMessages() async {
    try {
      final api = ref.read(apiClientProvider);
      final messages = await api.getChatMessages(limit: 200);
      final artifacts = await api.getArtifacts().catchError((_) => <Map<String, dynamic>>[]);
      print('[REFRESH] server returned ${messages.length} messages');

      // Preserve optimistic messages not yet stored on server
      final serverKeys = messages.map((m) => '${m['seq']}_${m['role']}').toSet();
      final optimistic = _messages.where((m) =>
        !serverKeys.contains('${m['seq']}_${m['role']}') &&
        (m['_thinking'] == true || m['status'] == 'pending')
      ).toList();
      if (optimistic.isNotEmpty) {
        print('[REFRESH] preserving ${optimistic.length} optimistic: ${optimistic.map((m) => '${m['seq']}_${m['role']}_thinking=${m['_thinking']}').join(', ')}');
      }
      messages.addAll(optimistic);

      // Sort: by seq, then user before assistant/task within same seq
      messages.sort((a, b) {
        final seqA = (a['seq'] as num?)?.toInt() ?? 0;
        final seqB = (b['seq'] as num?)?.toInt() ?? 0;
        if (seqA != seqB) return seqA.compareTo(seqB);
        // Within same seq: user first, then assistant/task
        final roleOrder = {'user': 0, 'assistant': 1, 'task': 1};
        return (roleOrder[a['role']] ?? 2).compareTo(roleOrder[b['role']] ?? 2);
      });
      if (mounted) {
        setState(() {
          _messages = messages;
          _artifacts = artifacts;
        });
      }
    } catch (_) {}
  }

  Future<void> _search(String query) async {
    if (query.isEmpty) {
      setState(() { _searchQuery = ''; _searchResults = null; });
      return;
    }
    try {
      final results = await ref.read(apiClientProvider).searchChatMessages(query);
      setState(() { _searchQuery = query; _searchResults = results; });
    } catch (_) {}
  }

  Future<void> _sendTask() async {
    final text = _inputController.text.trim();
    if (text.isEmpty) return;
    _inputController.clear();
    _inputFocusNode.requestFocus();

    final optimisticSeq = (_messages.isEmpty ? 1 : (_messages.last['seq'] as num).toInt() + 1);
    setState(() {
      _messages.add({
        'seq': optimisticSeq, 'role': 'user', 'content': '/task $text',
        'status': 'complete', 'timestamp': DateTime.now().toIso8601String(),
      });
      _messages.add({
        'seq': optimisticSeq, 'role': 'task', 'content': '',
        'status': 'pending', 'timestamp': DateTime.now().toIso8601String(),
        '_thinking': true,
      });
    });
    _scrollToBottom();

    try {
      final result = await ref.read(apiClientProvider).submitTask(text, model: _selectedModel);
      final jobId = result['job_id'] as String?;
      if (jobId != null) _pendingJobs[jobId] = optimisticSeq;
    } catch (e) {
      setState(() {
        _messages.removeWhere((m) => m['seq'] == optimisticSeq && m['_thinking'] == true);
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Task failed: $e')));
      }
    }
  }

  Future<void> _sendMessage() async {
    final text = _inputController.text.trim();
    if (text.isEmpty) return;

    // Check for /task prefix
    if (text.startsWith('/task ')) {
      _inputController.text = text.substring(6);
      return _sendTask();
    }

    _inputController.clear();
    _inputFocusNode.requestFocus();

    // Optimistic seq (will be corrected by server)
    final optimisticSeq = (_messages.isEmpty ? 1 : (_messages.last['seq'] as num).toInt() + 1);

    // Add user message immediately
    setState(() {
      _messages.add({
        'seq': optimisticSeq,
        'role': 'user',
        'content': text,
        'status': 'complete',
        'timestamp': DateTime.now().toIso8601String(),
      });
      // Add assistant placeholder
      _messages.add({
        'seq': optimisticSeq,
        'role': 'assistant',
        'content': '',
        'status': 'pending',
        'timestamp': DateTime.now().toIso8601String(),
        '_thinking': true,
      });
    });
    _scrollToBottom();

    try {
      final api = ref.read(apiClientProvider);
      final result = await api.sendChatMessage(text, model: _selectedModel);
      final jobId = result['job_id'] as String?;
      final seq = (result['seq'] as num?)?.toInt() ?? optimisticSeq;

      if (jobId != null) {
        _pendingJobs[jobId] = seq;
        print('[SEND] jobId=$jobId seq=$seq pending=${_pendingJobs.length}');
      }
    } catch (e) {
      // Remove the optimistic messages
      setState(() {
        _messages.removeWhere((m) => m['seq'] == optimisticSeq && m['_thinking'] == true);
        _messages.removeWhere((m) => m['seq'] == optimisticSeq && m['role'] == 'user' && m['content'] == text);
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to send: $e')),
        );
      }
    }
  }

  /// Persistent SSE connection to the chat stream. Stays open for the
  /// lifetime of the screen — no per-message reconnection (which caused
  /// events to be lost during the reconnect gap). Seq matching routes
  /// text chunks to the correct message.
  void _connectChatStream() {
    _streamSub?.cancel();
    print('[STREAM] connecting...');
    final api = ref.read(apiClientProvider);
    final streamUrl = api.chatStreamUrl;

    final dio = Dio(BaseOptions(extra: {'withCredentials': true}));
    dio.get<ResponseBody>(
      streamUrl,
      options: Options(responseType: ResponseType.stream, headers: {'Accept': 'text/event-stream'}),
    ).then((response) {
      final stream = response.data?.stream;
      if (stream == null) return;

      String buffer = '';
      _streamSub = stream.listen(
        (chunk) {
          if (!mounted) return;
          buffer += utf8.decode(chunk);

          while (buffer.contains('\n\n')) {
            final idx = buffer.indexOf('\n\n');
            final block = buffer.substring(0, idx);
            buffer = buffer.substring(idx + 2);

            String? data;
            for (final line in block.split('\n')) {
              if (line.startsWith('data: ')) data = line.substring(6);
            }
            if (data == null) continue;

            try {
              final parsed = json.decode(data) as Map<String, dynamic>;
              final eventType = parsed['type'] as String?;
              final eventSeq = (parsed['seq'] as num?)?.toInt();

              if (eventType == 'thinking' && parsed['content'] != null) {
                setState(() {
                  final seq = eventSeq ?? 0;
                  _thinkingContent[seq] = (_thinkingContent[seq] ?? '') + (parsed['content'] as String);
                  // Update the message bubble to show thinking
                  for (int i = _messages.length - 1; i >= 0; i--) {
                    final msg = _messages[i];
                    if (msg['role'] != 'assistant') continue;
                    final msgSeq = (msg['seq'] as num?)?.toInt();
                    if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
                    if (msg['_thinking'] == true || (eventSeq != null && msgSeq == eventSeq)) {
                      msg['_thinkingText'] = _thinkingContent[seq];
                      break;
                    }
                  }
                });
                _scrollToBottom();
              } else if (eventType == 'tool_use') {
                final tool = parsed['tool'] as String? ?? '';
                final summary = parsed['input_summary'] as String? ?? '';
                setState(() {
                  _toolStatus = _formatToolActivity(tool, summary);
                  for (int i = _messages.length - 1; i >= 0; i--) {
                    final msg = _messages[i];
                    if (msg['role'] != 'assistant') continue;
                    final msgSeq = (msg['seq'] as num?)?.toInt();
                    if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
                    if (msg['_thinking'] == true || (eventSeq != null && msgSeq == eventSeq)) {
                      msg['_toolStatus'] = _toolStatus;
                      break;
                    }
                  }
                });
              } else if (eventType == 'text' && parsed['content'] != null) {
                setState(() {
                  _toolStatus = '';
                  for (int i = _messages.length - 1; i >= 0; i--) {
                    final msg = _messages[i];
                    if (msg['role'] != 'assistant') continue;
                    final msgSeq = (msg['seq'] as num?)?.toInt();
                    if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
                    if (msg['_thinking'] == true || (eventSeq != null && msgSeq == eventSeq)) {
                      msg['content'] = (msg['content'] as String? ?? '') + parsed['content'];
                      msg['_thinking'] = false;
                      msg['_toolStatus'] = null;
                      break;
                    }
                  }
                });
                _scrollToBottom();
              } else if (eventType == 'cancelled') {
                setState(() {
                  _cancelling = false;
                  _toolStatus = '';
                  for (int i = _messages.length - 1; i >= 0; i--) {
                    final msg = _messages[i];
                    if (msg['_thinking'] == true || msg['status'] == 'pending') {
                      msg['_thinking'] = false;
                      final existing = msg['content'] as String? ?? '';
                      msg['content'] = existing.isEmpty ? '[Cancelled]' : '$existing\n\n_[Cancelled]_';
                      break;
                    }
                  }
                  _pendingJobs.clear();
                  _thinkingContent.clear();
                });
              } else if (eventType == 'done') {
                final doneSeq = eventSeq;
                final jobId = _pendingJobs.entries
                    .where((e) => e.value == doneSeq)
                    .map((e) => e.key)
                    .firstOrNull;
                if (jobId != null) {
                  _pendingJobs.remove(jobId);
                  ref.read(apiClientProvider).getChat().then((session) {
                    if (mounted) setState(() => _session = session);
                  }).catchError((_) {});
                }
                setState(() {
                  _cancelling = false;
                  _toolStatus = '';
                  if (doneSeq != null) _thinkingContent.remove(doneSeq);
                });
              }
            } catch (_) {}
          }
        },
        onDone: () {
          // SSE connection closed — reconnect after a short delay
          if (mounted) {
            Future.delayed(const Duration(seconds: 2), () {
              if (mounted) _connectChatStream();
            });
          }
        },
        onError: (_) {
          if (mounted) {
            Future.delayed(const Duration(seconds: 2), () {
              if (mounted) _connectChatStream();
            });
          }
        },
      );
    }).catchError((_) {
      // Connection failed — retry
      if (mounted) {
        Future.delayed(const Duration(seconds: 2), () {
          if (mounted) _connectChatStream();
        });
      }
    });
  }

  Future<void> _onJobComplete(String jobId) async {
    print('[JOB_COMPLETE] jobId=$jobId via SSE event, pending=${_pendingJobs.length}');
    _pendingJobs.remove(jobId);
    await _refreshMessages();
    try {
      final api = ref.read(apiClientProvider);
      final session = await api.getChat();
      if (mounted) setState(() => _session = session);
    } catch (_) {}
    _scrollToBottom();
  }

  Future<void> _retryMessage(int seq) async {
    try {
      final api = ref.read(apiClientProvider);
      final userMsg = _messages.where((m) => m['seq'] == seq && m['role'] == 'user').firstOrNull;
      if (userMsg == null) return;

      await api.retryChatMessage(seq);
      await api.sendChatMessage(userMsg['content'] as String, model: _selectedModel);
      await _refreshMessages();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Retry failed: $e')),
        );
      }
    }
  }

  String _formatToolActivity(String tool, String summary) {
    switch (tool) {
      case 'Read': return 'Reading ${summary.isNotEmpty ? summary : "file"}';
      case 'Write': return 'Writing ${summary.isNotEmpty ? summary : "file"}';
      case 'Edit': return 'Editing ${summary.isNotEmpty ? summary : "file"}';
      case 'Bash': return 'Running ${summary.isNotEmpty ? summary : "command"}';
      case 'Grep': return 'Searching ${summary.isNotEmpty ? summary : "files"}';
      case 'Glob': return 'Finding files';
      default: return summary.isNotEmpty ? '$tool: $summary' : tool;
    }
  }

  Future<void> _cancelMessage() async {
    setState(() => _cancelling = true);
    try {
      await ref.read(apiClientProvider).cancelChat();
    } catch (e) {
      setState(() => _cancelling = false);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Cancel failed: $e')),
        );
      }
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.animateTo(
          _scrollController.position.maxScrollExtent,
          duration: const Duration(milliseconds: 200),
          curve: Curves.easeOut,
        );
      }
    });
  }

  void _showArtifactViewer(Map<String, dynamic> artifact) async {
    final api = ref.read(apiClientProvider);
    final id = (artifact['id'] as num).toInt();
    try {
      final full = await api.getArtifact(id);
      final content = full['content'] as String? ?? '';
      final filename = full['filename'] as String? ?? 'snippet';
      final language = full['language'] as String? ?? '';
      if (!mounted) return;
      showDialog(
        context: context,
        builder: (ctx) => AlertDialog(
          title: Row(
            children: [
              Expanded(child: Text(filename, style: const TextStyle(fontSize: 16))),
              if (language.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: Chip(
                    label: Text(language, style: const TextStyle(fontSize: 11)),
                    visualDensity: VisualDensity.compact,
                  ),
                ),
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                tooltip: 'Copy',
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: content));
                  ScaffoldMessenger.of(ctx).showSnackBar(
                    const SnackBar(content: Text('Copied to clipboard')),
                  );
                },
              ),
            ],
          ),
          content: SizedBox(
            width: 700,
            height: 500,
            child: SingleChildScrollView(
              child: MarkdownMessage(content: '```$language\n$content\n```'),
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
          ],
        ),
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to load artifact: $e')),
        );
      }
    }
  }

  void _showArtifactsPanel() {
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Artifacts (${_artifacts.length})'),
        content: SizedBox(
          width: 500,
          height: 400,
          child: _artifacts.isEmpty
              ? const Center(child: Text('No artifacts yet'))
              : ListView.builder(
                  itemCount: _artifacts.length,
                  itemBuilder: (context, index) {
                    final a = _artifacts[index];
                    final filename = a['filename'] as String? ?? 'snippet';
                    final language = a['language'] as String? ?? '';
                    final lines = (a['lines'] as num?)?.toInt() ?? 0;
                    final seq = (a['seq'] as num?)?.toInt() ?? 0;
                    return ListTile(
                      leading: const Icon(Icons.code),
                      title: Text(filename),
                      subtitle: Text('$language \u00b7 $lines lines \u00b7 message #$seq'),
                      dense: true,
                      onTap: () {
                        Navigator.pop(ctx);
                        _showArtifactViewer(a);
                      },
                    );
                  },
                ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_error != null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline, size: 48, color: Theme.of(context).colorScheme.error),
            const SizedBox(height: 16),
            Text(_error!),
            const SizedBox(height: 16),
            FilledButton(onPressed: _initChat, child: const Text('Retry')),
          ],
        ),
      );
    }

    final totalCost = (_session?['total_cost_usd'] as num?)?.toDouble() ?? 0.0;
    final messageCount = _messages.length;
    final hasPending = _pendingJobs.isNotEmpty;

    return Column(
      children: [
        // Header
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: Theme.of(context).dividerColor)),
          ),
          child: Row(
            children: [
              const Icon(Icons.chat, size: 20),
              const SizedBox(width: 8),
              Semantics(
                header: true,
                label: 'Interactive Chat',
                child: Text('Interactive Chat',
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.bold)),
              ),
              const SizedBox(width: 16),
              Text('$messageCount messages', style: Theme.of(context).textTheme.bodySmall),
              if (totalCost > 0) ...[
                const SizedBox(width: 16),
                Text('\$${totalCost.toStringAsFixed(4)}', style: Theme.of(context).textTheme.bodySmall),
              ],
              const Spacer(),
              // Search
              SizedBox(
                width: 200,
                height: 32,
                child: TextField(
                  decoration: InputDecoration(
                    hintText: 'Search...',
                    prefixIcon: const Icon(Icons.search, size: 16),
                    border: OutlineInputBorder(borderRadius: BorderRadius.circular(16)),
                    contentPadding: EdgeInsets.zero,
                    isDense: true,
                  ),
                  style: const TextStyle(fontSize: 13),
                  onSubmitted: _search,
                ),
              ),
              const SizedBox(width: 8),
              TextButton.icon(
                icon: const Icon(Icons.download, size: 16),
                label: const Text('Export'),
                onPressed: () {
                  final api = ref.read(apiClientProvider);
                  launchUrl(Uri.parse(api.chatExportUrl));
                },
              ),
              if (_artifacts.isNotEmpty)
                TextButton.icon(
                  icon: Badge(
                    label: Text('${_artifacts.length}', style: const TextStyle(fontSize: 10)),
                    child: const Icon(Icons.code, size: 16),
                  ),
                  label: const Text('Artifacts'),
                  onPressed: _showArtifactsPanel,
                ),
              if (_session?['workspace_id'] != null)
                TextButton.icon(
                  icon: const Icon(Icons.folder_open, size: 16),
                  label: const Text('Workspace'),
                  onPressed: () {
                    final wsId = _session!['workspace_id'] as String;
                    context.go('/workspaces/$wsId');
                  },
                ),
            ],
          ),
        ),

        // Search results banner
        if (_searchResults != null) ...[
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 8),
            color: Theme.of(context).colorScheme.surfaceContainerHighest,
            child: Row(
              children: [
                Text('${_searchResults!.length} results for "$_searchQuery"',
                    style: Theme.of(context).textTheme.bodySmall),
                const Spacer(),
                TextButton(onPressed: () => setState(() { _searchQuery = ''; _searchResults = null; }),
                    child: const Text('Clear')),
              ],
            ),
          ),
        ],

        // Messages
        Expanded(
          child: _messages.isEmpty
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.chat_bubble_outline, size: 64, color: Colors.grey.shade600),
                      const SizedBox(height: 16),
                      Text('Start a conversation',
                          style: Theme.of(context).textTheme.titleMedium?.copyWith(color: Colors.grey.shade500)),
                      const SizedBox(height: 8),
                      Text('Your chat history persists across sessions.',
                          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Colors.grey.shade600)),
                    ],
                  ),
                )
              : ListView.builder(
                  controller: _scrollController,
                  padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
                  itemCount: _messages.length,
                  itemBuilder: (context, index) {
                    final msg = _messages[index];
                    final isAssistant = msg['role'] == 'assistant';
                    final seq = (msg['seq'] as num?)?.toInt() ?? 0;
                    final isPending = msg['_thinking'] == true || msg['status'] == 'pending';
                    final isStillStreaming = _pendingJobs.values.contains(seq);
                    final msgArtifacts = _artifacts.where(
                      (a) => (a['seq'] as num?)?.toInt() == seq,
                    ).toList();
                    return _MessageBubble(
                      message: msg,
                      isThinking: isPending && (msg['content'] as String? ?? '').isEmpty,
                      isStreaming: isStillStreaming,
                      thinkingText: msg['_thinkingText'] as String?,
                      toolStatus: msg['_toolStatus'] as String?,
                      onRetry: isAssistant && !isPending && seq > 0
                          ? () => _retryMessage(seq)
                          : null,
                      artifacts: isAssistant ? msgArtifacts : const [],
                      onArtifactTap: _showArtifactViewer,
                    );
                  },
                ),
        ),

        // Input bar — ALWAYS enabled (non-blocking)
        Container(
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            border: Border(top: BorderSide(color: Theme.of(context).dividerColor)),
          ),
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 900),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      Expanded(
                        child: Focus(
                          onKeyEvent: (node, event) {
                            if (event is KeyDownEvent &&
                                event.logicalKey == LogicalKeyboardKey.enter &&
                                !HardwareKeyboard.instance.isShiftPressed) {
                              _sendMessage();
                              return KeyEventResult.handled; // Consume Enter — no newline
                            }
                            return KeyEventResult.ignored; // Let Shift+Enter through as newline
                          },
                          child: TextField(
                            controller: _inputController,
                            focusNode: _inputFocusNode,
                            autofocus: true,
                            maxLines: 5,
                            minLines: 1,
                            decoration: InputDecoration(
                              hintText: hasPending ? 'Type another message...' : 'Type a message...',
                              border: const OutlineInputBorder(),
                              contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                            ),
                          ),
                        ),
                      ),
                      const SizedBox(width: 8),
                      SizedBox(
                        height: 48,
                        child: FilledButton.icon(
                          onPressed: _sendMessage,
                          icon: const Icon(Icons.send),
                          label: const Text('Send'),
                        ),
                      ),
                      if (!hasPending) ...[
                        const SizedBox(width: 4),
                        SizedBox(
                          height: 48,
                          child: OutlinedButton.icon(
                            onPressed: _sendTask,
                            icon: const Icon(Icons.construction, size: 18),
                            label: const Text('Task'),
                          ),
                        ),
                      ],
                      if (hasPending) ...[
                        const SizedBox(width: 4),
                        SizedBox(
                          height: 48,
                          child: OutlinedButton.icon(
                            onPressed: _cancelling ? null : _cancelMessage,
                            icon: Icon(_cancelling ? Icons.hourglass_top : Icons.stop, size: 18, color: _cancelling ? null : Colors.red),
                            label: Text(_cancelling ? 'Cancelling...' : 'Stop'),
                            style: OutlinedButton.styleFrom(
                              foregroundColor: _cancelling ? null : Colors.red,
                              side: _cancelling ? null : const BorderSide(color: Colors.red),
                            ),
                          ),
                        ),
                      ],
                    ],
                  ),
                  const SizedBox(height: 8),
                  Row(
                    children: [
                      const Icon(Icons.smart_toy, size: 14, color: Colors.grey),
                      const SizedBox(width: 4),
                      SegmentedButton<String>(
                        segments: const [
                          ButtonSegment(value: 'haiku', label: Text('Haiku')),
                          ButtonSegment(value: 'sonnet', label: Text('Sonnet')),
                          ButtonSegment(value: 'opus', label: Text('Opus')),
                        ],
                        selected: {_selectedModel},
                        onSelectionChanged: (v) => setState(() => _selectedModel = v.first),
                        style: ButtonStyle(
                          visualDensity: VisualDensity.compact,
                          textStyle: WidgetStatePropertyAll(Theme.of(context).textTheme.bodySmall),
                        ),
                      ),
                      if (hasPending) ...[
                        const SizedBox(width: 12),
                        SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 2, color: Colors.grey.shade500)),
                        const SizedBox(width: 6),
                        Text('${_pendingJobs.length} pending', style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Colors.grey.shade500)),
                      ],
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _MessageBubble extends StatefulWidget {
  final Map<String, dynamic> message;
  final bool isThinking;
  final bool isStreaming;
  final String? thinkingText;
  final String? toolStatus;
  final VoidCallback? onRetry;
  final List<Map<String, dynamic>> artifacts;
  final void Function(Map<String, dynamic>)? onArtifactTap;

  const _MessageBubble({required this.message, this.isThinking = false, this.isStreaming = false, this.thinkingText, this.toolStatus, this.onRetry, this.artifacts = const [], this.onArtifactTap});

  @override
  State<_MessageBubble> createState() => _MessageBubbleState();
}

class _MessageBubbleState extends State<_MessageBubble> {
  bool _thinkingExpanded = false;

  @override
  Widget build(BuildContext context) {
    final role = widget.message['role'] as String? ?? 'user';
    final content = widget.message['content'] as String? ?? '';
    final isUser = role == 'user';
    final isTask = role == 'task';
    final cost = (widget.message['cost_usd'] as num?)?.toDouble();
    final filesWritten = (widget.message['files_written'] as List?)?.cast<String>() ?? [];
    final storedThinking = widget.message['thinking'] as String?;
    final hasThinkingContent = (widget.thinkingText ?? storedThinking ?? '').isNotEmpty;
    final hasContent = content.isNotEmpty;

    final IconData icon;
    final Color bgColor;
    final String label;
    if (isUser) {
      icon = Icons.person; bgColor = Theme.of(context).colorScheme.primary; label = 'You';
    } else if (isTask) {
      icon = Icons.construction; bgColor = Colors.orange; label = 'Task';
    } else {
      icon = Icons.smart_toy; bgColor = Theme.of(context).colorScheme.secondary; label = 'Claude';
    }

    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          CircleAvatar(
            radius: 16,
            backgroundColor: bgColor,
            child: Icon(icon, size: 18, color: Colors.white),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Text(
                      label,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(fontWeight: FontWeight.bold),
                    ),
                    const SizedBox(width: 8),
                    if (cost != null && cost > 0)
                      Text(
                        '\$${cost.toStringAsFixed(4)}',
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Colors.grey.shade500,
                          fontSize: 11,
                        ),
                      ),
                  ],
                ),
                const SizedBox(height: 4),
                // Thinking section: expanded while streaming, collapsible after done
                if (!isUser && hasThinkingContent) ...[
                  if (widget.isStreaming || widget.isThinking)
                    // Still streaming — keep thinking visible above the response
                    _buildThinkingSection(context, widget.thinkingText ?? storedThinking ?? '', expanded: true)
                  else
                    // Message complete — collapsible toggle
                    _buildThinkingSection(context, widget.thinkingText ?? storedThinking ?? '', expanded: _thinkingExpanded),
                ],
                // Tool activity line
                if (!isUser && widget.toolStatus != null && widget.toolStatus!.isNotEmpty && !hasContent)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Row(
                      children: [
                        SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 2, color: Colors.grey.shade500)),
                        const SizedBox(width: 8),
                        Flexible(
                          child: Text(
                            widget.toolStatus!,
                            style: TextStyle(color: Colors.grey.shade500, fontSize: 12, fontStyle: FontStyle.italic),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                      ],
                    ),
                  ),
                // Spinner: only if nothing else is showing yet
                if (widget.isThinking && !hasContent && !hasThinkingContent && widget.toolStatus == null)
                  Row(
                    children: [
                      SizedBox(
                        width: 14, height: 14,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: Theme.of(context).colorScheme.secondary,
                        ),
                      ),
                      const SizedBox(width: 8),
                      Text('Thinking...', style: TextStyle(
                        color: Colors.grey.shade500,
                        fontStyle: FontStyle.italic,
                      )),
                    ],
                  ),
                // Response text
                if (hasContent)
                  Semantics(
                    label: '${isUser ? "You" : "Claude"}: $content',
                    child: isUser
                        ? SelectableText(content, style: const TextStyle(height: 1.5))
                        : MarkdownMessage(content: content),
                  ),
                if (filesWritten.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    children: filesWritten.map((f) => Chip(
                      avatar: const Icon(Icons.insert_drive_file, size: 14),
                      label: Text(f, style: const TextStyle(fontSize: 12)),
                      visualDensity: VisualDensity.compact,
                    )).toList(),
                  ),
                ],
                if (widget.artifacts.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    children: widget.artifacts.map((a) => ActionChip(
                      avatar: const Icon(Icons.code, size: 14),
                      label: Text(
                        '${a['filename'] ?? 'snippet'} (${a['lines']} lines)',
                        style: const TextStyle(fontSize: 12),
                      ),
                      visualDensity: VisualDensity.compact,
                      onPressed: () => widget.onArtifactTap?.call(a),
                    )).toList(),
                  ),
                ],
              ],
            ),
          ),
          if (!isUser && !widget.isThinking && content.isNotEmpty)
            Column(
              children: [
                IconButton(
                  icon: const Icon(Icons.copy, size: 16),
                  tooltip: 'Copy',
                  visualDensity: VisualDensity.compact,
                  constraints: const BoxConstraints(minWidth: 28, minHeight: 28),
                  padding: EdgeInsets.zero,
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: content));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Copied'), duration: Duration(seconds: 1)),
                    );
                  },
                ),
                if (widget.onRetry != null)
                  IconButton(
                    icon: const Icon(Icons.refresh, size: 16),
                    tooltip: 'Retry',
                    visualDensity: VisualDensity.compact,
                    constraints: const BoxConstraints(minWidth: 28, minHeight: 28),
                    padding: EdgeInsets.zero,
                    onPressed: widget.onRetry,
                  ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _buildThinkingSection(BuildContext context, String thinkingText, {required bool expanded}) {
    final content = widget.message['content'] as String? ?? '';
    final hasResponse = content.isNotEmpty;

    if (!hasResponse) {
      // Still streaming thinking — show full text
      return Container(
        margin: const EdgeInsets.only(bottom: 8),
        padding: const EdgeInsets.all(8),
        decoration: BoxDecoration(
          color: Colors.grey.shade900.withValues(alpha: 0.3),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.psychology, size: 14, color: Colors.grey.shade500),
                const SizedBox(width: 4),
                Text('Thinking', style: TextStyle(color: Colors.grey.shade500, fontSize: 11, fontWeight: FontWeight.bold)),
              ],
            ),
            const SizedBox(height: 4),
            Text(
              thinkingText.length > 500 ? '...${thinkingText.substring(thinkingText.length - 500)}' : thinkingText,
              style: TextStyle(color: Colors.grey.shade400, fontSize: 12, fontStyle: FontStyle.italic, height: 1.4),
            ),
          ],
        ),
      );
    }

    // Response arrived — collapsible toggle
    return GestureDetector(
      onTap: () => setState(() => _thinkingExpanded = !_thinkingExpanded),
      child: Container(
        margin: const EdgeInsets.only(bottom: 8),
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: Colors.grey.shade900.withValues(alpha: 0.2),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.psychology, size: 14, color: Colors.grey.shade500),
                const SizedBox(width: 4),
                Text('Thinking', style: TextStyle(color: Colors.grey.shade500, fontSize: 11, fontWeight: FontWeight.bold)),
                const Spacer(),
                Icon(expanded ? Icons.expand_less : Icons.expand_more, size: 16, color: Colors.grey.shade500),
              ],
            ),
            if (expanded) ...[
              const SizedBox(height: 4),
              Text(
                thinkingText,
                style: TextStyle(color: Colors.grey.shade400, fontSize: 12, fontStyle: FontStyle.italic, height: 1.4),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
