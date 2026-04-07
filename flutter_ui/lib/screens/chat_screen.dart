import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:url_launcher/url_launcher.dart';
import '../main.dart';
import '../providers/chat_controller.dart';
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
  String _selectedModel = 'sonnet';
  String _searchQuery = '';
  List<Map<String, dynamic>>? _searchResults;
  bool _userScrolledUp = false;
  int _lastSeenStreamTick = -1;

  @override
  void initState() {
    super.initState();
    _scrollController.addListener(_onScroll);
    // Force scroll to bottom on first build (after layout completes).
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _pinToBottom(force: true);
    });
  }

  @override
  void dispose() {
    _inputController.dispose();
    _inputFocusNode.dispose();
    _scrollController.removeListener(_onScroll);
    _scrollController.dispose();
    super.dispose();
  }

  void _onScroll() {
    if (!_scrollController.hasClients) return;
    // In `reverse: true` mode, "at bottom" means scroll position near 0.
    final atBottom = _scrollController.position.pixels < 100;
    final scrolledUp = !atBottom;
    if (_userScrolledUp != scrolledUp) {
      setState(() => _userScrolledUp = scrolledUp);
    }
  }

  /// Pin the view to the newest message. With `reverse: true`, that's
  /// scroll offset 0. `force: true` overrides the user-scrolled-up guard.
  void _pinToBottom({bool force = false}) {
    if (!force && _userScrolledUp) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_scrollController.hasClients) return;
      _scrollController.jumpTo(0);
    });
  }

  Future<void> _send() async {
    final text = _inputController.text.trim();
    if (text.isEmpty) return;
    _inputController.clear();
    _inputFocusNode.requestFocus();
    // User-initiated send always pins to bottom.
    _pinToBottom(force: true);
    await ref
        .read(chatControllerProvider.notifier)
        .sendMessage(text, model: _selectedModel);
  }

  Future<void> _sendTask() async {
    final text = _inputController.text.trim();
    if (text.isEmpty) return;
    _inputController.clear();
    _inputFocusNode.requestFocus();
    _pinToBottom(force: true);
    await ref
        .read(chatControllerProvider.notifier)
        .sendTask(text, model: _selectedModel);
  }

  Future<void> _search(String query) async {
    if (query.isEmpty) {
      setState(() {
        _searchQuery = '';
        _searchResults = null;
      });
      return;
    }
    try {
      final results = await ref.read(apiClientProvider).searchChatMessages(query);
      setState(() {
        _searchQuery = query;
        _searchResults = results;
      });
    } catch (_) {}
  }

  Future<void> _showArtifactViewer(Map<String, dynamic> artifact) async {
    final api = ref.read(apiClientProvider);
    final id = (artifact['id'] as num).toInt();
    try {
      final full = await api.getArtifact(id);
      final content = full['content'] as String? ?? '';
      final filename = full['filename'] as String? ?? 'snippet';
      final language = full['language'] as String? ?? '';
      final isBinary = full['binary'] == true;
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
                icon: const Icon(Icons.download, size: 18),
                tooltip: 'Download',
                onPressed: () => _downloadArtifact(id, filename),
              ),
              if (!isBinary)
                IconButton(
                  icon: const Icon(Icons.copy, size: 18),
                  tooltip: 'Copy',
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: content));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                        content: Text('Copied'),
                        duration: Duration(seconds: 1),
                      ),
                    );
                  },
                ),
            ],
          ),
          content: SizedBox(
            width: 700,
            height: 500,
            child: SingleChildScrollView(
              child: isBinary
                  ? Center(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          const Icon(Icons.insert_drive_file, size: 64),
                          const SizedBox(height: 12),
                          Text(content),
                          const SizedBox(height: 12),
                          FilledButton.icon(
                            icon: const Icon(Icons.download, size: 18),
                            label: const Text('Download'),
                            onPressed: () => _downloadArtifact(id, filename),
                          ),
                        ],
                      ),
                    )
                  : MarkdownMessage(content: content),
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
          ],
        ),
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to load artifact: $e')));
      }
    }
  }

  Future<void> _downloadArtifact(int id, String filename) async {
    try {
      await ref.read(apiClientProvider).downloadArtifact(id, filename);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Download failed: $e')));
      }
    }
  }

  void _showArtifactsPanel(List<Map<String, dynamic>> artifacts) {
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Artifacts (${artifacts.length})'),
        content: SizedBox(
          width: 500,
          height: 400,
          child: artifacts.isEmpty
              ? const Center(child: Text('No artifacts yet'))
              : ListView.builder(
                  itemCount: artifacts.length,
                  itemBuilder: (context, index) {
                    final a = artifacts[index];
                    final filename = a['filename'] as String? ?? 'snippet';
                    final language = a['language'] as String? ?? '';
                    final lines = (a['lines'] as num?)?.toInt() ?? 0;
                    final seq = (a['seq'] as num?)?.toInt() ?? 0;
                    final id = (a['id'] as num).toInt();
                    return ListTile(
                      leading: const Icon(Icons.code),
                      title: Text(filename),
                      subtitle: Text('$language \u00b7 $lines lines \u00b7 message #$seq'),
                      dense: true,
                      trailing: IconButton(
                        icon: const Icon(Icons.download, size: 18),
                        tooltip: 'Download',
                        onPressed: () => _downloadArtifact(id, filename),
                      ),
                      onTap: () {
                        Navigator.pop(ctx);
                        _showArtifactViewer(a);
                      },
                    );
                  },
                ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final chat = ref.watch(chatControllerProvider);
    final controller = ref.read(chatControllerProvider.notifier);

    // Forward controller errors to a snackbar.
    ref.listen<String?>(
      chatControllerProvider.select((s) => s.error),
      (prev, next) {
        if (next != null && next != prev) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(next)));
        }
      },
    );

    // Subscribe to the broadcast error stream too (rolled-back optimistic
    // sends, etc., which don't change `state.error`).
    ref.listen<AsyncValue<String>>(
      _chatErrorStreamProvider,
      (_, next) {
        next.whenData((msg) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text(msg)));
        });
      },
    );

    // Auto-pin to bottom whenever new content arrives, unless the user has
    // scrolled up. The controller bumps `streamTick` after every flush.
    if (controller.streamTick != _lastSeenStreamTick) {
      _lastSeenStreamTick = controller.streamTick;
      _pinToBottom();
    }

    if (chat.loading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (chat.error != null && chat.messages.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline,
                size: 48, color: Theme.of(context).colorScheme.error),
            const SizedBox(height: 16),
            Text(chat.error!),
            const SizedBox(height: 16),
            FilledButton(
                onPressed: controller.loadInitial,
                child: const Text('Retry')),
          ],
        ),
      );
    }

    final messages = chat.messages;
    final pendingJobs = chat.pendingJobs;
    final artifacts = chat.artifacts;
    final session = chat.session;
    final cancelling = chat.cancelling;
    final totalCost = (session?['total_cost_usd'] as num?)?.toDouble() ?? 0.0;
    final hasPending = pendingJobs.isNotEmpty;

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
                child: Text(
                  'Interactive Chat',
                  style: Theme.of(context)
                      .textTheme
                      .titleMedium
                      ?.copyWith(fontWeight: FontWeight.bold),
                ),
              ),
              const SizedBox(width: 16),
              Text('${messages.length} messages',
                  style: Theme.of(context).textTheme.bodySmall),
              if (totalCost > 0) ...[
                const SizedBox(width: 16),
                Text('\$${totalCost.toStringAsFixed(4)}',
                    style: Theme.of(context).textTheme.bodySmall),
              ],
              const Spacer(),
              SizedBox(
                width: 200,
                height: 32,
                child: TextField(
                  decoration: InputDecoration(
                    hintText: 'Search...',
                    prefixIcon: const Icon(Icons.search, size: 16),
                    border:
                        OutlineInputBorder(borderRadius: BorderRadius.circular(16)),
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
              if (artifacts.isNotEmpty)
                TextButton.icon(
                  icon: Badge(
                    label: Text('${artifacts.length}',
                        style: const TextStyle(fontSize: 10)),
                    child: const Icon(Icons.code, size: 16),
                  ),
                  label: const Text('Artifacts'),
                  onPressed: () => _showArtifactsPanel(artifacts),
                ),
              if (session?['workspace_id'] != null)
                TextButton.icon(
                  icon: const Icon(Icons.folder_open, size: 16),
                  label: const Text('Workspace'),
                  onPressed: () {
                    final wsId = session!['workspace_id'] as String;
                    context.go('/workspaces/$wsId');
                  },
                ),
            ],
          ),
        ),

        // Search results banner
        if (_searchResults != null)
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 8),
            color: Theme.of(context).colorScheme.surfaceContainerHighest,
            child: Row(
              children: [
                Text('${_searchResults!.length} results for "$_searchQuery"',
                    style: Theme.of(context).textTheme.bodySmall),
                const Spacer(),
                TextButton(
                  onPressed: () => setState(() {
                    _searchQuery = '';
                    _searchResults = null;
                  }),
                  child: const Text('Clear'),
                ),
              ],
            ),
          ),

        // Messages
        Expanded(
          child: messages.isEmpty
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.chat_bubble_outline,
                          size: 64, color: Colors.grey.shade600),
                      const SizedBox(height: 16),
                      Text(
                        'Start a conversation',
                        style: Theme.of(context)
                            .textTheme
                            .titleMedium
                            ?.copyWith(color: Colors.grey.shade500),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'Your chat history persists across sessions.',
                        style: Theme.of(context)
                            .textTheme
                            .bodySmall
                            ?.copyWith(color: Colors.grey.shade600),
                      ),
                    ],
                  ),
                )
              : Stack(
                  children: [
                    ListView.builder(
                      controller: _scrollController,
                      reverse: true,
                      padding: const EdgeInsets.symmetric(
                          horizontal: 24, vertical: 16),
                      itemCount: messages.length,
                      itemBuilder: (context, index) {
                        // reverse: true → index 0 is the newest message
                        final msg = messages[messages.length - 1 - index];
                        final isAssistant = msg['role'] == 'assistant';
                        final seq = (msg['seq'] as num?)?.toInt() ?? 0;
                        final isPending = msg['_thinking'] == true ||
                            msg['status'] == 'pending';
                        final isStillStreaming =
                            pendingJobs.values.contains(seq);
                        final msgArtifacts = artifacts
                            .where((a) =>
                                (a['seq'] as num?)?.toInt() == seq)
                            .toList();
                        return _MessageBubble(
                          message: msg,
                          isThinking: isPending &&
                              (msg['content'] as String? ?? '').isEmpty,
                          isStreaming: isStillStreaming,
                          thinkingText: msg['_thinkingText'] as String?,
                          toolStatus: msg['_toolStatus'] as String?,
                          onRetry: isAssistant && !isPending && seq > 0
                              ? () => controller.retryMessage(seq,
                                  model: _selectedModel)
                              : null,
                          artifacts: isAssistant ? msgArtifacts : const [],
                          onArtifactTap: _showArtifactViewer,
                          onArtifactDownload: (a) => _downloadArtifact(
                              (a['id'] as num).toInt(),
                              a['filename'] as String? ?? 'artifact'),
                        );
                      },
                    ),
                    // "New messages ↓" floating button when the user has
                    // scrolled away from the bottom and content is growing.
                    if (_userScrolledUp)
                      Positioned(
                        bottom: 12,
                        right: 24,
                        child: FloatingActionButton.small(
                          tooltip: 'Jump to latest',
                          onPressed: () => _pinToBottom(force: true),
                          child: const Icon(Icons.arrow_downward),
                        ),
                      ),
                  ],
                ),
        ),

        // Input bar
        Container(
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            border:
                Border(top: BorderSide(color: Theme.of(context).dividerColor)),
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
                              _send();
                              return KeyEventResult.handled;
                            }
                            return KeyEventResult.ignored;
                          },
                          child: TextField(
                            controller: _inputController,
                            focusNode: _inputFocusNode,
                            autofocus: true,
                            maxLines: 5,
                            minLines: 1,
                            decoration: InputDecoration(
                              hintText: hasPending
                                  ? 'Type another message...'
                                  : 'Type a message...',
                              border: const OutlineInputBorder(),
                              contentPadding: const EdgeInsets.symmetric(
                                  horizontal: 16, vertical: 12),
                            ),
                          ),
                        ),
                      ),
                      const SizedBox(width: 8),
                      SizedBox(
                        height: 48,
                        child: FilledButton.icon(
                          onPressed: _send,
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
                            onPressed: cancelling
                                ? null
                                : controller.cancelMessage,
                            icon: Icon(
                                cancelling ? Icons.hourglass_top : Icons.stop,
                                size: 18,
                                color: cancelling ? null : Colors.red),
                            label: Text(cancelling ? 'Cancelling...' : 'Stop'),
                            style: OutlinedButton.styleFrom(
                              foregroundColor: cancelling ? null : Colors.red,
                              side: cancelling
                                  ? null
                                  : const BorderSide(color: Colors.red),
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
                        onSelectionChanged: (v) =>
                            setState(() => _selectedModel = v.first),
                        style: ButtonStyle(
                          visualDensity: VisualDensity.compact,
                          textStyle: WidgetStatePropertyAll(
                              Theme.of(context).textTheme.bodySmall),
                        ),
                      ),
                      if (hasPending) ...[
                        const SizedBox(width: 12),
                        SizedBox(
                            width: 12,
                            height: 12,
                            child: CircularProgressIndicator(
                                strokeWidth: 2, color: Colors.grey.shade500)),
                        const SizedBox(width: 6),
                        Text('${pendingJobs.length} pending',
                            style: Theme.of(context)
                                .textTheme
                                .bodySmall
                                ?.copyWith(color: Colors.grey.shade500)),
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

/// Wraps the controller's broadcast error stream as an `AsyncValue<String>`
/// so the screen can use `ref.listen` to drive snackbars.
final _chatErrorStreamProvider = StreamProvider.autoDispose<String>((ref) {
  final controller = ref.watch(chatControllerProvider.notifier);
  return controller.errorStream;
});

class _MessageBubble extends StatefulWidget {
  final Map<String, dynamic> message;
  final bool isThinking;
  final bool isStreaming;
  final String? thinkingText;
  final String? toolStatus;
  final VoidCallback? onRetry;
  final List<Map<String, dynamic>> artifacts;
  final void Function(Map<String, dynamic>)? onArtifactTap;
  final void Function(Map<String, dynamic>)? onArtifactDownload;

  const _MessageBubble({
    required this.message,
    this.isThinking = false,
    this.isStreaming = false,
    this.thinkingText,
    this.toolStatus,
    this.onRetry,
    this.artifacts = const [],
    this.onArtifactTap,
    this.onArtifactDownload,
  });

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
    final filesWritten =
        (widget.message['files_written'] as List?)?.cast<String>() ?? [];
    final storedThinking = widget.message['thinking'] as String?;
    final hasThinkingContent =
        (widget.thinkingText ?? storedThinking ?? '').isNotEmpty;
    final hasContent = content.isNotEmpty;

    final IconData icon;
    final Color bgColor;
    final String label;
    if (isUser) {
      icon = Icons.person;
      bgColor = Theme.of(context).colorScheme.primary;
      label = 'You';
    } else if (isTask) {
      icon = Icons.construction;
      bgColor = Colors.orange;
      label = 'Task';
    } else {
      icon = Icons.smart_toy;
      bgColor = Theme.of(context).colorScheme.secondary;
      label = 'Claude';
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
                      style: Theme.of(context)
                          .textTheme
                          .bodySmall
                          ?.copyWith(fontWeight: FontWeight.bold),
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
                if (!isUser && hasThinkingContent) ...[
                  if (widget.isStreaming || widget.isThinking)
                    _buildThinkingSection(
                        context, widget.thinkingText ?? storedThinking ?? '',
                        expanded: true)
                  else
                    _buildThinkingSection(
                        context, widget.thinkingText ?? storedThinking ?? '',
                        expanded: _thinkingExpanded),
                ],
                if (!isUser &&
                    widget.toolStatus != null &&
                    widget.toolStatus!.isNotEmpty &&
                    !hasContent)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Row(
                      children: [
                        SizedBox(
                            width: 12,
                            height: 12,
                            child: CircularProgressIndicator(
                                strokeWidth: 2, color: Colors.grey.shade500)),
                        const SizedBox(width: 8),
                        Flexible(
                          child: Text(
                            widget.toolStatus!,
                            style: TextStyle(
                                color: Colors.grey.shade500,
                                fontSize: 12,
                                fontStyle: FontStyle.italic),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                      ],
                    ),
                  ),
                if (widget.isThinking &&
                    !hasContent &&
                    !hasThinkingContent &&
                    widget.toolStatus == null)
                  Row(
                    children: [
                      SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: Theme.of(context).colorScheme.secondary,
                        ),
                      ),
                      const SizedBox(width: 8),
                      Text('Thinking...',
                          style: TextStyle(
                            color: Colors.grey.shade500,
                            fontStyle: FontStyle.italic,
                          )),
                    ],
                  ),
                if (hasContent)
                  Semantics(
                    label: '${isUser ? "You" : "Claude"}: $content',
                    child: isUser
                        ? SelectableText(content,
                            style: const TextStyle(height: 1.5))
                        : MarkdownMessage(content: content),
                  ),
                if (filesWritten.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    children: filesWritten
                        .map((f) => Chip(
                              avatar: const Icon(Icons.insert_drive_file,
                                  size: 14),
                              label:
                                  Text(f, style: const TextStyle(fontSize: 12)),
                              visualDensity: VisualDensity.compact,
                            ))
                        .toList(),
                  ),
                ],
                if (widget.artifacts.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    runSpacing: 4,
                    children: widget.artifacts
                        .map((a) => _ArtifactChip(
                              artifact: a,
                              onTap: () => widget.onArtifactTap?.call(a),
                              onDownload: () =>
                                  widget.onArtifactDownload?.call(a),
                            ))
                        .toList(),
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
                  constraints:
                      const BoxConstraints(minWidth: 28, minHeight: 28),
                  padding: EdgeInsets.zero,
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: content));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                          content: Text('Copied'),
                          duration: Duration(seconds: 1)),
                    );
                  },
                ),
                if (widget.onRetry != null)
                  IconButton(
                    icon: const Icon(Icons.refresh, size: 16),
                    tooltip: 'Retry',
                    visualDensity: VisualDensity.compact,
                    constraints:
                        const BoxConstraints(minWidth: 28, minHeight: 28),
                    padding: EdgeInsets.zero,
                    onPressed: widget.onRetry,
                  ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _buildThinkingSection(BuildContext context, String thinkingText,
      {required bool expanded}) {
    final content = widget.message['content'] as String? ?? '';
    final hasResponse = content.isNotEmpty;

    if (!hasResponse) {
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
                Text('Thinking',
                    style: TextStyle(
                        color: Colors.grey.shade500,
                        fontSize: 11,
                        fontWeight: FontWeight.bold)),
              ],
            ),
            const SizedBox(height: 4),
            Text(
              thinkingText.length > 500
                  ? '...${thinkingText.substring(thinkingText.length - 500)}'
                  : thinkingText,
              style: TextStyle(
                  color: Colors.grey.shade400,
                  fontSize: 12,
                  fontStyle: FontStyle.italic,
                  height: 1.4),
            ),
          ],
        ),
      );
    }

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
                Text('Thinking',
                    style: TextStyle(
                        color: Colors.grey.shade500,
                        fontSize: 11,
                        fontWeight: FontWeight.bold)),
                const Spacer(),
                Icon(expanded ? Icons.expand_less : Icons.expand_more,
                    size: 16, color: Colors.grey.shade500),
              ],
            ),
            if (expanded) ...[
              const SizedBox(height: 4),
              Text(
                thinkingText,
                style: TextStyle(
                    color: Colors.grey.shade400,
                    fontSize: 12,
                    fontStyle: FontStyle.italic,
                    height: 1.4),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Chip for an artifact attached to an assistant message. Two tap targets:
/// label opens the viewer, trailing icon downloads the file directly. The
/// existing `ActionChip` only supports one `onPressed` so we build it as a
/// custom `Material` + `InkWell` row.
class _ArtifactChip extends StatelessWidget {
  final Map<String, dynamic> artifact;
  final VoidCallback onTap;
  final VoidCallback onDownload;

  const _ArtifactChip({
    required this.artifact,
    required this.onTap,
    required this.onDownload,
  });

  @override
  Widget build(BuildContext context) {
    final filename = artifact['filename'] as String? ?? 'snippet';
    final lines = artifact['lines'];
    return Material(
      color: Theme.of(context).colorScheme.surfaceContainerHigh,
      borderRadius: BorderRadius.circular(16),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          InkWell(
            borderRadius: const BorderRadius.only(
              topLeft: Radius.circular(16),
              bottomLeft: Radius.circular(16),
            ),
            onTap: onTap,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(10, 6, 6, 6),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const Icon(Icons.code, size: 14),
                  const SizedBox(width: 6),
                  Text(
                    '$filename ($lines lines)',
                    style: const TextStyle(fontSize: 12),
                  ),
                ],
              ),
            ),
          ),
          InkWell(
            borderRadius: const BorderRadius.only(
              topRight: Radius.circular(16),
              bottomRight: Radius.circular(16),
            ),
            onTap: onDownload,
            child: const Padding(
              padding: EdgeInsets.fromLTRB(4, 6, 8, 6),
              child: Icon(Icons.download, size: 14),
            ),
          ),
        ],
      ),
    );
  }
}
