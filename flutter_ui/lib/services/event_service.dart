import 'dart:async';
import 'dart:convert';
import 'package:dio/dio.dart';

/// Service that connects to the SSE endpoint for real-time job updates.
class EventService {
  final String _baseUrl;
  final _controller = StreamController<Map<String, dynamic>>.broadcast();
  CancelToken? _cancelToken;
  bool _running = false;

  EventService(this._baseUrl);

  /// Stream of job update events from the server.
  Stream<Map<String, dynamic>> get jobUpdates => _controller.stream;

  /// Start listening for SSE events. Automatically reconnects on disconnect.
  void connect() {
    if (_running) return;
    _running = true;
    _listen();
  }

  void disconnect() {
    _running = false;
    _cancelToken?.cancel();
    _cancelToken = null;
  }

  void dispose() {
    disconnect();
    _controller.close();
  }

  Future<void> _listen() async {
    while (_running) {
      _cancelToken = CancelToken();
      try {
        final dio = Dio();
        final response = await dio.get<ResponseBody>(
          '$_baseUrl/api/v1/events/jobs',
          options: Options(
            responseType: ResponseType.stream,
            headers: {'Accept': 'text/event-stream'},
          ),
          cancelToken: _cancelToken,
        );

        final stream = response.data?.stream;
        if (stream == null) continue;

        String buffer = '';
        await for (final chunk in stream) {
          if (!_running) break;
          buffer += utf8.decode(chunk);

          // Parse SSE: lines separated by \n\n
          while (buffer.contains('\n\n')) {
            final idx = buffer.indexOf('\n\n');
            final block = buffer.substring(0, idx);
            buffer = buffer.substring(idx + 2);

            String? eventType;
            String? data;
            for (final line in block.split('\n')) {
              if (line.startsWith('event: ')) {
                eventType = line.substring(7);
              } else if (line.startsWith('data: ')) {
                data = line.substring(6);
              }
            }

            if (data != null && eventType == 'job_update') {
              try {
                final parsed = json.decode(data) as Map<String, dynamic>;
                _controller.add(parsed);
              } catch (_) {}
            }
          }
        }
      } on DioException catch (e) {
        if (e.type == DioExceptionType.cancel) break;
        // Connection lost — wait and retry
      } catch (_) {
        // Unexpected error — wait and retry
      }

      if (_running) {
        await Future.delayed(const Duration(seconds: 3));
      }
    }
  }
}
