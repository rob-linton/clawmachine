import 'dart:async';
import 'dart:convert';
import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;

import '../main.dart';
import '../services/api_client.dart';
import '../services/event_service.dart';
import '../widgets/tool_activity.dart';

/// Immutable snapshot of all chat-related state.
///
/// Lives inside [ChatController]. Individual message maps are still mutable
/// `Map<String, dynamic>` (so streaming chunks can be appended in place);
/// the controller assigns a new `messages` list reference whenever it
/// flushes pending updates so Riverpod fires a rebuild.
class ChatState {
  final List<Map<String, dynamic>> messages;
  final List<Map<String, dynamic>> artifacts;
  final Map<String, dynamic>? session;
  final Map<String, int> pendingJobs; // job_id → seq
  final Map<int, String> thinkingContent; // seq → accumulated thinking
  final String toolStatus;
  final bool cancelling;
  final bool loading;
  final String? error;

  const ChatState({
    this.messages = const [],
    this.artifacts = const [],
    this.session,
    this.pendingJobs = const {},
    this.thinkingContent = const {},
    this.toolStatus = '',
    this.cancelling = false,
    this.loading = true,
    this.error,
  });

  ChatState copyWith({
    List<Map<String, dynamic>>? messages,
    List<Map<String, dynamic>>? artifacts,
    Map<String, dynamic>? session,
    bool clearSession = false,
    Map<String, int>? pendingJobs,
    Map<int, String>? thinkingContent,
    String? toolStatus,
    bool? cancelling,
    bool? loading,
    String? error,
    bool clearError = false,
  }) {
    return ChatState(
      messages: messages ?? this.messages,
      artifacts: artifacts ?? this.artifacts,
      session: clearSession ? null : (session ?? this.session),
      pendingJobs: pendingJobs ?? this.pendingJobs,
      thinkingContent: thinkingContent ?? this.thinkingContent,
      toolStatus: toolStatus ?? this.toolStatus,
      cancelling: cancelling ?? this.cancelling,
      loading: loading ?? this.loading,
      error: clearError ? null : (error ?? this.error),
    );
  }
}

/// Owns the chat session state, SSE connection, polling fallback, and
/// jobUpdates subscription. Lives for the lifetime of the provider — survives
/// navigation away from `/chat` so the spinner / pending state are not lost.
class ChatController extends StateNotifier<ChatState> {
  final ApiClient _api;
  final EventService _events;

  StreamSubscription? _eventSub;
  Timer? _pollTimer;
  Timer? _flushTimer;
  bool _stopped = false;
  bool _streamRunning = false;
  int _reconnectDelaySecs = 2;
  bool _flushPending = false;
  final Map<String, DateTime> _lastJobCheck = {};

  // Snackbar/toast surface (no BuildContext available inside the controller).
  final StreamController<String> _errorController = StreamController.broadcast();
  Stream<String> get errorStream => _errorController.stream;

  // Bumped whenever the message list grows or the last message changes —
  // the screen watches this to drive auto-pin-to-bottom.
  int _streamTick = 0;
  int get streamTick => _streamTick;

  ChatController(this._api, this._events) : super(const ChatState()) {
    _init();
  }

  Future<void> _init() async {
    _connectStream();
    _eventSub = _events.jobUpdates.listen((event) {
      final jobId = event['job_id'] as String?;
      if (jobId != null && state.pendingJobs.containsKey(jobId)) {
        final status = event['status'] as String?;
        if (status == 'completed' || status == 'failed') {
          _onJobComplete(jobId);
        }
      }
    });
    // Polling fallback: SSE pub/sub is fire-and-forget, so cross-check any
    // outstanding pending jobs against `/jobs/{id}` and refresh stuck
    // optimistic messages periodically.
    _pollTimer = Timer.periodic(const Duration(seconds: 2), (_) => _poll());
    await loadInitial();
  }

  Future<void> loadInitial() async {
    state = state.copyWith(loading: true, clearError: true);
    try {
      final session = await _api.createOrGetChat();
      final messages = await _api.getChatMessages(limit: 100);
      final artifacts = await _api
          .getArtifacts()
          .catchError((_) => <Map<String, dynamic>>[]);
      state = state.copyWith(
        session: session,
        messages: messages,
        artifacts: artifacts,
        loading: false,
      );
      _bumpStreamTick();
    } catch (e) {
      state = state.copyWith(loading: false, error: 'Failed to load chat: $e');
    }
  }

  /// Refresh messages from the server. Server is authoritative — only
  /// preserve optimistic messages whose (seq, role) the server has not yet
  /// stored. Without this guard, a stuck `_thinking` flag would be re-added
  /// on every refresh and never clear.
  Future<void> refreshMessages() async {
    try {
      final serverMessages = await _api.getChatMessages(limit: 200);
      final artifacts = await _api
          .getArtifacts()
          .catchError((_) => <Map<String, dynamic>>[]);

      final serverKeys =
          serverMessages.map((m) => '${m['seq']}_${m['role']}').toSet();
      final optimistic = state.messages.where((m) {
        final key = '${m['seq']}_${m['role']}';
        if (serverKeys.contains(key)) return false; // server wins
        // Only preserve genuinely-in-flight optimistic placeholders.
        return m['_thinking'] == true || m['status'] == 'pending';
      }).toList();
      serverMessages.addAll(optimistic);

      // Carry forward transient client-side fields (`_activity`,
      // `_thinkingText`) from the previous in-memory message to the new
      // server-stored copy when they share the same (seq, role). Without
      // this, the inline activity timeline would vanish the moment
      // refreshMessages replaces the optimistic placeholder with the
      // server message — because the server doesn't store the per-event
      // activity log, only the final ChatMessage. Survives until a hard
      // page refresh; see plan §4 known limitations.
      final existingByKey = <String, Map<String, dynamic>>{};
      for (final m in state.messages) {
        existingByKey['${m['seq']}_${m['role']}'] = m;
      }
      for (final newMsg in serverMessages) {
        final existing = existingByKey['${newMsg['seq']}_${newMsg['role']}'];
        if (existing == null) continue;
        if (existing['_activity'] is List && newMsg['_activity'] == null) {
          newMsg['_activity'] = existing['_activity'];
        }
        if (existing['_thinkingText'] is String &&
            newMsg['_thinkingText'] == null) {
          newMsg['_thinkingText'] = existing['_thinkingText'];
        }
      }

      // Sort messages chronologically by stored timestamp. The server uses
      // chrono::Utc::now() (UTC, `Z` suffix) and the optimistic placeholders
      // below now also use .toUtc(). DateTime.parse handles both forms
      // correctly and compares them as instants — lex compare on raw
      // strings would fail if any optimistic timestamps were ever local.
      // Tie-break with seq then role priority for determinism when two
      // messages share an exact timestamp.
      serverMessages.sort((a, b) {
        final tsA = DateTime.tryParse(a['timestamp'] as String? ?? '');
        final tsB = DateTime.tryParse(b['timestamp'] as String? ?? '');
        if (tsA != null && tsB != null) {
          final cmp = tsA.compareTo(tsB);
          if (cmp != 0) return cmp;
        } else if (tsA == null && tsB != null) {
          return 1;
        } else if (tsA != null && tsB == null) {
          return -1;
        }
        final seqA = (a['seq'] as num?)?.toInt() ?? 0;
        final seqB = (b['seq'] as num?)?.toInt() ?? 0;
        if (seqA != seqB) return seqA.compareTo(seqB);
        final roleOrder = {'user': 0, 'assistant': 1, 'task': 1};
        return (roleOrder[a['role']] ?? 2).compareTo(roleOrder[b['role']] ?? 2);
      });

      state = state.copyWith(messages: serverMessages, artifacts: artifacts);
      _bumpStreamTick();
    } catch (_) {
      // Polling errors are silent — next tick will try again.
    }
  }

  Future<void> sendMessage(String text, {String model = 'sonnet'}) async {
    if (text.isEmpty) return;
    if (text.startsWith('/task ')) {
      return sendTask(text.substring(6), model: model);
    }

    final optimisticSeq = state.messages.isEmpty
        ? 1
        : (state.messages.last['seq'] as num).toInt() + 1;
    // UTC timestamps so the time-ordered sort in refreshMessages compares
    // apples-to-apples against server-side chrono::Utc::now() values.
    final nowUtc = DateTime.now().toUtc().toIso8601String();
    final newMessages = List<Map<String, dynamic>>.from(state.messages)
      ..add({
        'seq': optimisticSeq,
        'role': 'user',
        'content': text,
        'status': 'complete',
        'timestamp': nowUtc,
      })
      ..add({
        'seq': optimisticSeq,
        'role': 'assistant',
        'content': '',
        'status': 'pending',
        'timestamp': DateTime.now().toUtc().toIso8601String(),
        '_thinking': true,
      });
    state = state.copyWith(messages: newMessages);
    _bumpStreamTick();

    try {
      final result = await _api.sendChatMessage(text, model: model);
      final jobId = result['job_id'] as String?;
      final seq = (result['seq'] as num?)?.toInt() ?? optimisticSeq;
      // Patch optimistic seq → real seq so isStillStreaming can match.
      if (seq != optimisticSeq) {
        for (final m in state.messages) {
          if (m['seq'] == optimisticSeq) m['seq'] = seq;
        }
      }
      if (jobId != null) {
        state = state.copyWith(
          pendingJobs: {...state.pendingJobs, jobId: seq},
        );
      }
    } catch (e) {
      // Roll back optimistic placeholder.
      final rolled = state.messages
          .where((m) =>
              !(m['seq'] == optimisticSeq && m['_thinking'] == true) &&
              !(m['seq'] == optimisticSeq &&
                  m['role'] == 'user' &&
                  m['content'] == text))
          .toList();
      state = state.copyWith(messages: rolled);
      _errorController.add('Failed to send: $e');
    }
  }

  Future<void> sendTask(String text, {String model = 'sonnet'}) async {
    if (text.isEmpty) return;
    final optimisticSeq = state.messages.isEmpty
        ? 1
        : (state.messages.last['seq'] as num).toInt() + 1;
    final nowUtc = DateTime.now().toUtc().toIso8601String();
    final newMessages = List<Map<String, dynamic>>.from(state.messages)
      ..add({
        'seq': optimisticSeq,
        'role': 'user',
        'content': '/task $text',
        'status': 'complete',
        'timestamp': nowUtc,
      })
      ..add({
        'seq': optimisticSeq,
        'role': 'task',
        'content': '',
        'status': 'pending',
        'timestamp': DateTime.now().toUtc().toIso8601String(),
        '_thinking': true,
      });
    state = state.copyWith(messages: newMessages);
    _bumpStreamTick();

    try {
      final result = await _api.submitTask(text, model: model);
      final jobId = result['job_id'] as String?;
      if (jobId != null) {
        state = state.copyWith(
          pendingJobs: {...state.pendingJobs, jobId: optimisticSeq},
        );
      }
    } catch (e) {
      final rolled = state.messages
          .where((m) => !(m['seq'] == optimisticSeq && m['_thinking'] == true))
          .toList();
      state = state.copyWith(messages: rolled);
      _errorController.add('Task failed: $e');
    }
  }

  Future<void> retryMessage(int seq, {String model = 'sonnet'}) async {
    try {
      final userMsg =
          state.messages.where((m) => m['seq'] == seq && m['role'] == 'user').firstOrNull;
      if (userMsg == null) return;
      await _api.retryChatMessage(seq);
      await _api.sendChatMessage(userMsg['content'] as String, model: model);
      await refreshMessages();
    } catch (e) {
      _errorController.add('Retry failed: $e');
    }
  }

  Future<void> cancelMessage() async {
    state = state.copyWith(cancelling: true);
    try {
      await _api.cancelChat();
    } catch (e) {
      state = state.copyWith(cancelling: false);
      _errorController.add('Cancel failed: $e');
    }
  }

  Future<void> _onJobComplete(String jobId) async {
    final newPending = Map<String, int>.from(state.pendingJobs)..remove(jobId);
    state = state.copyWith(pendingJobs: newPending);
    await refreshMessages();
    try {
      final session = await _api.getChat();
      state = state.copyWith(session: session);
    } catch (_) {}
  }

  // ---- Polling fallback --------------------------------------------------

  Future<void> _poll() async {
    if (_stopped) return;
    final hasThinking = state.messages.any((m) => m['_thinking'] == true);
    final hasEmptyAssistant = state.messages.any((m) =>
        m['role'] == 'assistant' && (m['content'] as String? ?? '').isEmpty);
    if (state.pendingJobs.isEmpty && !hasThinking && !hasEmptyAssistant) {
      return;
    }

    // Cross-check each pending job against /jobs/{id} (throttled per-job).
    final now = DateTime.now();
    for (final entry in state.pendingJobs.entries.toList()) {
      final last = _lastJobCheck[entry.key];
      if (last != null && now.difference(last).inSeconds < 5) continue;
      _lastJobCheck[entry.key] = now;
      try {
        final job = await _api.getJob(entry.key);
        final s = job.status.toLowerCase();
        if (s == 'completed' || s == 'failed' || s == 'cancelled') {
          await _onJobComplete(entry.key);
        }
      } catch (_) {
        // Ignore — next tick retries.
      }
    }

    await refreshMessages();
  }

  // ---- SSE stream --------------------------------------------------------

  void _connectStream() {
    if (_streamRunning || _stopped) return;
    _streamRunning = true;
    _fetchSSE(_api.chatStreamUrl);
  }

  Future<void> _fetchSSE(String url) async {
    try {
      final init = web.RequestInit(
        method: 'GET',
        headers: {'Accept': 'text/event-stream'}.jsify() as web.HeadersInit,
        credentials: 'include',
      );
      final response = await web.window.fetch(url.toJS, init).toDart;

      if (response.status == 401 || response.status == 403) {
        _streamRunning = false;
        _errorController.add('Chat stream unauthorized — please log in again');
        return; // Do NOT reconnect on auth failure.
      }
      if (!response.ok) {
        _streamRunning = false;
        _scheduleReconnect();
        return;
      }
      _reconnectDelaySecs = 2; // success — reset backoff

      final body = response.body;
      if (body == null) {
        _streamRunning = false;
        _scheduleReconnect();
        return;
      }

      final reader = body.getReader();
      final decoder = web.TextDecoder();
      String buffer = '';

      while (!_stopped) {
        final result =
            await (reader.callMethod<JSPromise>('read'.toJS)).toDart;
        final done =
            (result as JSObject).getProperty<JSBoolean>('done'.toJS).toDart;
        if (done) break;
        final value = result.getProperty<JSObject>('value'.toJS);
        final chunk = decoder.decode(value, web.TextDecodeOptions(stream: true));
        buffer += chunk;

        while (buffer.contains('\n\n')) {
          final idx = buffer.indexOf('\n\n');
          final block = buffer.substring(0, idx);
          buffer = buffer.substring(idx + 2);

          String? data;
          for (final line in block.split('\n')) {
            if (line.startsWith('data: ')) data = line.substring(6);
          }
          if (data == null) continue;
          _handleSSEData(data);
        }
      }
    } catch (_) {
      // Fall through to reconnect.
    }
    _streamRunning = false;
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    if (_stopped) return;
    final delay = _reconnectDelaySecs;
    // Exponential backoff capped at 30s.
    _reconnectDelaySecs = (_reconnectDelaySecs * 2).clamp(2, 30);
    Future.delayed(Duration(seconds: delay), () {
      if (!_stopped) _connectStream();
    });
  }

  void _handleSSEData(String data) {
    try {
      final parsed = json.decode(data) as Map<String, dynamic>;
      final eventType = parsed['type'] as String?;
      final eventSeq = (parsed['seq'] as num?)?.toInt();

      if (eventType == 'thinking' && parsed['content'] != null) {
        final seq = eventSeq ?? 0;
        final newThinking = Map<int, String>.from(state.thinkingContent);
        newThinking[seq] = (newThinking[seq] ?? '') + (parsed['content'] as String);
        for (int i = state.messages.length - 1; i >= 0; i--) {
          final msg = state.messages[i];
          if (msg['role'] != 'assistant') continue;
          final msgSeq = (msg['seq'] as num?)?.toInt();
          if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
          if (msg['_thinking'] == true ||
              (eventSeq != null && msgSeq == eventSeq)) {
            msg['_thinkingText'] = newThinking[seq];
            break;
          }
        }
        state = state.copyWith(thinkingContent: newThinking);
        // Thinking events are infrequent (Claude emits them in batches at
        // reasoning checkpoints, not per-token) so the 50ms coalesce buys
        // nothing — flush immediately for prompt visibility.
        _flushNow();
      } else if (eventType == 'tool_use') {
        final tool = parsed['tool'] as String? ?? '';
        final summary = parsed['input_summary'] as String? ?? '';
        final toolUseId = parsed['tool_use_id'] as String? ?? '';
        final toolStatus = toolSummaryHumanized(tool, summary);
        for (int i = state.messages.length - 1; i >= 0; i--) {
          final msg = state.messages[i];
          if (msg['role'] != 'assistant') continue;
          final msgSeq = (msg['seq'] as num?)?.toInt();
          if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
          if (msg['_thinking'] == true ||
              (eventSeq != null && msgSeq == eventSeq)) {
            msg['_toolStatus'] = toolStatus;
            // Accumulate the call into the per-message activity timeline.
            final newActivity = List<Map<String, dynamic>>.from(
                (msg['_activity'] as List?)?.cast<Map<String, dynamic>>() ??
                    const []);
            newActivity.add({
              'type': 'tool_use',
              'tool': tool,
              'summary': summary,
              'tool_use_id': toolUseId,
            });
            msg['_activity'] = newActivity;
            break;
          }
        }
        state = state.copyWith(toolStatus: toolStatus);
        // Use _scheduleFlush (NOT _flushNow) so tool events don't fight
        // text-event coalescing during a streaming turn — _flushNow would
        // cancel the in-flight text coalesce timer on every tool call.
        _scheduleFlush();
      } else if (eventType == 'tool_result') {
        // Each tool_use's result. Match to the originating tool_use by
        // tool_use_id and append into the same per-message activity list.
        final toolUseId = parsed['tool_use_id'] as String? ?? '';
        final output = parsed['output'] as String? ?? '';
        final truncated = parsed['truncated'] == true;
        final isError = parsed['is_error'] == true;
        for (int i = state.messages.length - 1; i >= 0; i--) {
          final msg = state.messages[i];
          if (msg['role'] != 'assistant') continue;
          final msgSeq = (msg['seq'] as num?)?.toInt();
          if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
          // Match the same way the tool_use branch does — by seq, or by
          // _thinking flag for the in-flight optimistic placeholder. The
          // earlier "has _activity" disjunct could pick the wrong message
          // if the latest assistant has no activity yet but a previous one
          // does.
          if (msg['_thinking'] == true ||
              (eventSeq != null && msgSeq == eventSeq)) {
            final newActivity = List<Map<String, dynamic>>.from(
                (msg['_activity'] as List?)?.cast<Map<String, dynamic>>() ??
                    const []);
            newActivity.add({
              'type': 'tool_result',
              'tool_use_id': toolUseId,
              'output': output,
              'truncated': truncated,
              'is_error': isError,
            });
            msg['_activity'] = newActivity;
            break;
          }
        }
        _scheduleFlush();
      } else if (eventType == 'text' && parsed['content'] != null) {
        for (int i = state.messages.length - 1; i >= 0; i--) {
          final msg = state.messages[i];
          if (msg['role'] != 'assistant') continue;
          final msgSeq = (msg['seq'] as num?)?.toInt();
          if (eventSeq != null && msgSeq != null && eventSeq != msgSeq) continue;
          if (msg['_thinking'] == true ||
              (eventSeq != null && msgSeq == eventSeq)) {
            msg['content'] = (msg['content'] as String? ?? '') + parsed['content'];
            msg['_thinking'] = false;
            msg['_toolStatus'] = null;
            break;
          }
        }
        state = state.copyWith(toolStatus: '');
        _scheduleFlush();
      } else if (eventType == 'tool_build') {
        final buildMsg = parsed['content'] as String? ?? 'Building tools...';
        for (int i = state.messages.length - 1; i >= 0; i--) {
          final msg = state.messages[i];
          if (msg['_thinking'] == true || msg['status'] == 'pending') {
            msg['_toolStatus'] = buildMsg;
            break;
          }
        }
        state = state.copyWith(toolStatus: buildMsg);
        _flushNow();
      } else if (eventType == 'cancelled') {
        for (int i = state.messages.length - 1; i >= 0; i--) {
          final msg = state.messages[i];
          if (msg['_thinking'] == true || msg['status'] == 'pending') {
            msg['_thinking'] = false;
            final existing = msg['content'] as String? ?? '';
            msg['content'] =
                existing.isEmpty ? '[Cancelled]' : '$existing\n\n_[Cancelled]_';
            // Mark any unfinished tool_use entries as cancelled so the
            // activity timeline can replace their in-flight spinner with a
            // cancelled indicator instead of leaving them stuck spinning.
            final activity = (msg['_activity'] as List?)
                ?.cast<Map<String, dynamic>>();
            if (activity != null) {
              final completedIds = activity
                  .where((e) => e['type'] == 'tool_result')
                  .map((e) => e['tool_use_id'])
                  .toSet();
              for (final entry in activity) {
                if (entry['type'] == 'tool_use' &&
                    !completedIds.contains(entry['tool_use_id'])) {
                  entry['cancelled'] = true;
                }
              }
            }
            break;
          }
        }
        state = state.copyWith(
          cancelling: false,
          toolStatus: '',
          pendingJobs: {},
          thinkingContent: {},
        );
        _flushNow();
      } else if (eventType == 'done') {
        final doneSeq = eventSeq;
        final newPending = Map<String, int>.from(state.pendingJobs);
        String? jobIdToRemove;
        for (final entry in newPending.entries) {
          if (entry.value == doneSeq) {
            jobIdToRemove = entry.key;
            break;
          }
        }
        if (jobIdToRemove != null) {
          newPending.remove(jobIdToRemove);
          // Refresh session metadata (cost) in the background.
          _api.getChat().then((session) {
            if (!_stopped) state = state.copyWith(session: session);
          }).catchError((_) {});
        }
        // Garbage-collect thinkingContent for completed seqs.
        final newThinking = Map<int, String>.from(state.thinkingContent);
        if (doneSeq != null) newThinking.remove(doneSeq);
        // Cap thinkingContent at the last 20 seqs to bound memory.
        if (newThinking.length > 20) {
          final sorted = newThinking.keys.toList()..sort();
          for (final k in sorted.take(newThinking.length - 20)) {
            newThinking.remove(k);
          }
        }
        state = state.copyWith(
          pendingJobs: newPending,
          cancelling: false,
          toolStatus: '',
          thinkingContent: newThinking,
        );
        _flushNow();
      }
    } catch (_) {
      // Malformed event — ignore.
    }
  }

  // ---- Flush coalescing --------------------------------------------------
  //
  // SSE `text` events arrive token-by-token. Without coalescing, every token
  // would trigger a full Riverpod rebuild of the chat screen. Buffer the
  // updates with a 50ms timer so we rebuild at most ~20Hz.

  void _scheduleFlush() {
    if (_flushPending) return;
    _flushPending = true;
    _flushTimer?.cancel();
    _flushTimer = Timer(const Duration(milliseconds: 50), _flushNow);
  }

  void _flushNow() {
    _flushPending = false;
    _flushTimer?.cancel();
    // Assigning a new list reference triggers Riverpod's state-change
    // notification even though the inner Map<String, dynamic> instances are
    // mutated in place above.
    state = state.copyWith(messages: List<Map<String, dynamic>>.from(state.messages));
    _bumpStreamTick();
  }

  void _bumpStreamTick() {
    _streamTick++;
  }

  @override
  void dispose() {
    _stopped = true;
    _eventSub?.cancel();
    _pollTimer?.cancel();
    _flushTimer?.cancel();
    _errorController.close();
    super.dispose();
  }
}

/// Long-lived chat controller. Survives navigation away from `/chat`.
/// Re-created when the authenticated user changes (`currentUserProvider`).
final chatControllerProvider =
    StateNotifierProvider<ChatController, ChatState>((ref) {
  // Invalidate the controller on logout / user switch.
  ref.watch(currentUserProvider);
  final api = ref.watch(apiClientProvider);
  final events = ref.watch(eventServiceProvider);
  return ChatController(api, events);
});
