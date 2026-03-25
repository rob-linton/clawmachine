import 'dart:async';
import 'dart:convert';
import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../main.dart';

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
  bool _sending = false;
  String? _error;
  String? _pendingJobId;
  StreamSubscription? _eventSub;

  @override
  void initState() {
    super.initState();
    _initChat();
    // Listen for job completion via SSE
    _eventSub = ref.read(eventServiceProvider).jobUpdates.listen((event) {
      if (_pendingJobId != null && event['job_id'] == _pendingJobId) {
        final status = event['status'] as String?;
        if (status == 'completed' || status == 'failed') {
          _onJobComplete();
        }
      }
    });
  }

  @override
  void dispose() {
    _inputController.dispose();
    _scrollController.dispose();
    _eventSub?.cancel();
    _cancelStream();
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

  Future<void> _sendMessage() async {
    final text = _inputController.text.trim();
    if (text.isEmpty || _sending) return;

    _inputController.clear();
    setState(() => _sending = true);

    // Optimistically add user message
    final optimisticMsg = {
      'seq': (_messages.isEmpty ? 1 : (_messages.last['seq'] as num).toInt() + 1),
      'role': 'user',
      'content': text,
      'timestamp': DateTime.now().toIso8601String(),
    };
    setState(() => _messages.add(optimisticMsg));
    _scrollToBottom();

    try {
      final api = ref.read(apiClientProvider);
      final result = await api.sendChatMessage(text);
      final jobId = result['job_id'] as String?;

      // Add a "thinking" placeholder
      setState(() {
        _messages.add({
          'seq': optimisticMsg['seq'],
          'role': 'assistant',
          'content': '',
          'job_id': jobId,
          'timestamp': DateTime.now().toIso8601String(),
          '_thinking': true,
        });
      });
      _scrollToBottom();

      // Subscribe to chat stream for progressive text display
      _pendingJobId = jobId;
      _startStreamSubscription();
    } catch (e) {
      setState(() {
        _sending = false;
        _pendingJobId = null;
        _messages.removeLast(); // remove optimistic
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to send: $e')),
        );
      }
    }
  }

  StreamSubscription? _streamSub;

  void _startStreamSubscription() {
    _streamSub?.cancel();
    final api = ref.read(apiClientProvider);
    final streamUrl = api.chatStreamUrl;

    // Connect to SSE chat stream for progressive text
    final dio = Dio(BaseOptions(extra: {'withCredentials': true}));
    dio.get<ResponseBody>(
      streamUrl,
      options: Options(responseType: ResponseType.stream, headers: {'Accept': 'text/event-stream'}),
    ).then((response) {
      final stream = response.data?.stream;
      if (stream == null) return;

      String buffer = '';
      _streamSub = stream.listen((chunk) {
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
              // Append text to the last (assistant) message
              setState(() {
                if (_messages.isNotEmpty) {
                  final last = _messages.last;
                  if (last['role'] == 'assistant') {
                    last['content'] = (last['content'] as String? ?? '') + parsed['content'];
                    last['_thinking'] = false;
                  }
                }
              });
              _scrollToBottom();
            }
          } catch (_) {}
        }
      });
    }).catchError((_) {
      // Stream connection failed — fall back to SSE job event completion
    });
  }

  void _cancelStream() {
    _streamSub?.cancel();
    _streamSub = null;
  }

  Future<void> _onJobComplete() async {
    _pendingJobId = null;
    _cancelStream();
    await _refreshMessages();
    // Refresh session for cost update
    try {
      final api = ref.read(apiClientProvider);
      final session = await api.getChat();
      if (mounted) setState(() => _session = session);
    } catch (_) {}
    if (mounted) setState(() => _sending = false);
    _scrollToBottom();
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
              if (_session?['workspace_id'] != null)
                TextButton.icon(
                  icon: const Icon(Icons.folder_open, size: 16),
                  label: const Text('Workspace'),
                  onPressed: () {
                    // Navigate to workspace file browser
                  },
                ),
            ],
          ),
        ),

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
                  itemBuilder: (context, index) => _MessageBubble(
                    message: _messages[index],
                    isThinking: _messages[index]['_thinking'] == true,
                  ),
                ),
        ),

        // Input bar
        Container(
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            border: Border(top: BorderSide(color: Theme.of(context).dividerColor)),
          ),
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 900),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  Expanded(
                    child: TextField(
                      controller: _inputController,
                      maxLines: 5,
                      minLines: 1,
                      enabled: !_sending,
                      decoration: InputDecoration(
                        hintText: _sending ? 'Waiting for response...' : 'Type a message...',
                        border: const OutlineInputBorder(),
                        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                      ),
                      onSubmitted: (_) => _sendMessage(),
                      inputFormatters: [
                        // Allow Shift+Enter for newlines, Enter to send
                        _EnterToSendFormatter(() => _sendMessage()),
                      ],
                    ),
                  ),
                  const SizedBox(width: 12),
                  SizedBox(
                    height: 48,
                    child: FilledButton.icon(
                      onPressed: _sending ? null : _sendMessage,
                      icon: _sending
                          ? const SizedBox(
                              width: 16, height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white))
                          : const Icon(Icons.send),
                      label: const Text('Send'),
                    ),
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

  const _MessageBubble({required this.message, this.isThinking = false});

  @override
  Widget build(BuildContext context) {
    final role = message['role'] as String? ?? 'user';
    final content = message['content'] as String? ?? '';
    final isUser = role == 'user';
    final cost = (message['cost_usd'] as num?)?.toDouble();
    final filesWritten = (message['files_written'] as List?)?.cast<String>() ?? [];

    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          CircleAvatar(
            radius: 16,
            backgroundColor: isUser
                ? Theme.of(context).colorScheme.primary
                : Theme.of(context).colorScheme.secondary,
            child: Icon(
              isUser ? Icons.person : Icons.smart_toy,
              size: 18,
              color: Colors.white,
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Text(
                      isUser ? 'You' : 'Claude',
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
                    child: SelectableText(
                      content,
                      style: const TextStyle(height: 1.5),
                    ),
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
        ],
      ),
    );
  }
}

/// Input formatter that sends on Enter (without Shift).
class _EnterToSendFormatter extends TextInputFormatter {
  final VoidCallback onSend;
  _EnterToSendFormatter(this.onSend);

  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) {
    // This is a simplified approach — the actual Shift+Enter detection
    // requires RawKeyboardListener. For now, just allow all input.
    // The onSubmitted callback handles Enter-to-send for single-line.
    return newValue;
  }
}
