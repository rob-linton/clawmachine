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
  final _scrollController = ScrollController();
  List<Map<String, dynamic>> _messages = [];
  Map<String, dynamic>? _session;
  bool _loading = true;
  String? _error;
  final Map<String, int> _pendingJobs = {}; // job_id → seq
  String _selectedModel = 'sonnet';
  String _searchQuery = '';
  List<Map<String, dynamic>>? _searchResults;
  StreamSubscription? _eventSub;
  StreamSubscription? _streamSub;

  @override
  void initState() {
    super.initState();
    _initChat();
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
  }

  @override
  void dispose() {
    _inputController.dispose();
    _scrollController.dispose();
    _eventSub?.cancel();
    _streamSub?.cancel();
    super.dispose();
  }

  Future<void> _initChat() async {
    setState(() => _loading = true);
    try {
      final api = ref.read(apiClientProvider);
      final session = await api.createOrGetChat();
      final messages = await api.getChatMessages(limit: 100);
      setState(() {
        _session = session;
        _messages = messages;
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
      if (mounted) {
        setState(() => _messages = messages);
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
        // Start streaming for this message
        _startStreamSubscription(jobId);
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

  void _startStreamSubscription(String jobId) {
    // Only one stream subscription at a time (chat messages are sequential)
    _streamSub?.cancel();
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
              if (parsed['type'] == 'text' && parsed['content'] != null) {
                // Find the latest pending assistant message and append text
                setState(() {
                  for (int i = _messages.length - 1; i >= 0; i--) {
                    if (_messages[i]['role'] == 'assistant' && _messages[i]['_thinking'] == true) {
                      _messages[i]['content'] = (_messages[i]['content'] as String? ?? '') + parsed['content'];
                      _messages[i]['_thinking'] = false; // Show text, not spinner
                      break;
                    }
                  }
                });
                _scrollToBottom();
              } else if (parsed['type'] == 'done') {
                _onJobComplete(jobId);
              }
            } catch (_) {}
          }
        },
        onDone: () {
          // Stream closed — give the worker time to store, then refresh
          if (mounted && _pendingJobs.containsKey(jobId)) {
            Future.delayed(const Duration(milliseconds: 1500), () {
              if (mounted && _pendingJobs.containsKey(jobId)) _onJobComplete(jobId);
            });
          }
        },
        onError: (_) {
          if (mounted && _pendingJobs.containsKey(jobId)) {
            Future.delayed(const Duration(milliseconds: 1500), () {
              if (mounted && _pendingJobs.containsKey(jobId)) _onJobComplete(jobId);
            });
          }
        },
      );
    }).catchError((_) {});
  }

  Future<void> _onJobComplete(String jobId) async {
    _pendingJobs.remove(jobId);
    _streamSub?.cancel();
    _streamSub = null;
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
                    return _MessageBubble(
                      message: msg,
                      isThinking: isPending && (msg['content'] as String? ?? '').isEmpty,
                      onRetry: isAssistant && !isPending && seq > 0
                          ? () => _retryMessage(seq)
                          : null,
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
                        child: TextField(
                          controller: _inputController,
                          maxLines: 5,
                          minLines: 1,
                          decoration: InputDecoration(
                            hintText: hasPending ? 'Type another message...' : 'Type a message...',
                            border: const OutlineInputBorder(),
                            contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                          ),
                          onSubmitted: (_) => _sendMessage(),
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

class _MessageBubble extends StatelessWidget {
  final Map<String, dynamic> message;
  final bool isThinking;
  final VoidCallback? onRetry;

  const _MessageBubble({required this.message, this.isThinking = false, this.onRetry});

  @override
  Widget build(BuildContext context) {
    final role = message['role'] as String? ?? 'user';
    final content = message['content'] as String? ?? '';
    final isUser = role == 'user';
    final isTask = role == 'task';
    final cost = (message['cost_usd'] as num?)?.toDouble();
    final filesWritten = (message['files_written'] as List?)?.cast<String>() ?? [];

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
                if (isThinking)
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
                  )
                else
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
              ],
            ),
          ),
          if (!isUser && !isThinking && content.isNotEmpty)
            Column(
              children: [
                IconButton(
                  icon: const Icon(Icons.copy, size: 16),
                  tooltip: 'Copy',
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: content));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Copied'), duration: Duration(seconds: 1)),
                    );
                  },
                ),
                if (onRetry != null)
                  IconButton(
                    icon: const Icon(Icons.refresh, size: 16),
                    tooltip: 'Retry',
                    onPressed: onRetry,
                  ),
              ],
            ),
        ],
      ),
    );
  }
}
