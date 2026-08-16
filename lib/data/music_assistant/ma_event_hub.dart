import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'ma_models.dart';

class MusicAssistantQueueEvent {
  const MusicAssistantQueueEvent({
    required this.event,
    required this.queueId,
    this.queue,
    this.rawData,
    required this.receivedAt,
  });

  final String event;
  final String? queueId;
  final PlayerQueue? queue;
  final Map<String, dynamic>? rawData;
  final DateTime receivedAt;
}

class MusicAssistantEventHub {
  MusicAssistantEventHub({
    required this.baseUrl,
    required this.authToken,
  });

  static const Set<String> _queueEventsFilter = <String>{
    'queue_added',
    'queue_removed',
    'queue_updated',
    'queue_items_updated',
    'queue_time_updated',
    'queue_settings_updated',
  };

  final String baseUrl;
  final String authToken;

  final StreamController<MusicAssistantQueueEvent> _queueEventController =
      StreamController<MusicAssistantQueueEvent>.broadcast();

  WebSocket? _socket;
  StreamSubscription<dynamic>? _socketSubscription;
  Completer<void>? _authCompleter;
  String? _authMessageId;
  bool _disposed = false;

  bool _connected = false;
  bool get isConnected => _connected;

  Stream<MusicAssistantQueueEvent> get queueEvents => _queueEventController.stream;

  Future<void> connect() async {
    if (_disposed) {
      throw StateError('MusicAssistantEventHub wurde bereits disposed.');
    }

    if (_connected || (_authCompleter != null && !_authCompleter!.isCompleted)) {
      return;
    }

    final token = authToken.trim();
    if (token.isEmpty) {
      throw StateError('MusicAssistantEventHub kann ohne Auth-Token nicht verbinden.');
    }

    final uri = _buildWebSocketUri(baseUrl);
    final socket = await WebSocket.connect(uri.toString());

    _socket = socket;
    _authCompleter = Completer<void>();
    _authMessageId = 'auth-${DateTime.now().microsecondsSinceEpoch}';

    _socketSubscription = socket.listen(
      _handleSocketMessage,
      onDone: _handleSocketDone,
      onError: _handleSocketError,
      cancelOnError: false,
    );

    final authPayload = <String, dynamic>{
      'message_id': _authMessageId,
      'command': 'auth',
      'args': <String, dynamic>{'token': token},
    };
    socket.add(jsonEncode(authPayload));

    try {
      await _authCompleter!.future.timeout(const Duration(seconds: 10));
      _connected = true;
    } catch (_) {
      await disconnect();
      rethrow;
    }
  }

  Future<void> disconnect() async {
    _connected = false;

    await _socketSubscription?.cancel();
    _socketSubscription = null;

    final socket = _socket;
    _socket = null;

    _authMessageId = null;
    if (_authCompleter != null && !_authCompleter!.isCompleted) {
      _authCompleter!.completeError(StateError('Event-Hub Verbindung wurde beendet.'));
    }
    _authCompleter = null;

    if (socket != null) {
      try {
        await socket.close(WebSocketStatus.normalClosure, 'shutdown');
      } catch (_) {
        // Ignore close failures.
      }
    }
  }

  Future<void> dispose() async {
    if (_disposed) {
      return;
    }

    _disposed = true;
    await disconnect();
    await _queueEventController.close();
  }

  void _handleSocketMessage(dynamic rawMessage) {
    if (rawMessage is! String || rawMessage.trim().isEmpty) {
      return;
    }

    dynamic decoded;
    try {
      decoded = jsonDecode(rawMessage);
    } catch (_) {
      return;
    }

    if (decoded is! Map<String, dynamic>) {
      return;
    }

    final messageId = (decoded['message_id'] as String?)?.trim();
    if (_authCompleter != null && !(_authCompleter!.isCompleted)) {
      if (messageId == _authMessageId) {
        final result = decoded['result'];
        final authenticated = result is Map<String, dynamic> && result['authenticated'] == true;
        if (authenticated) {
          _authCompleter!.complete();
          return;
        }

        if (decoded.containsKey('error')) {
          _authCompleter!.completeError(
            StateError('WebSocket-Auth fehlgeschlagen: ${decoded['error']}'),
          );
          return;
        }
      }
    }

    final eventName = (decoded['event'] as String?)?.trim();
    if (eventName == null || !_queueEventsFilter.contains(eventName)) {
      return;
    }

    final queueId = (decoded['object_id'] as String?)?.trim();
    final data = decoded['data'];

    PlayerQueue? queue;
    Map<String, dynamic>? rawData;

    if (data is Map<String, dynamic>) {
      rawData = Map<String, dynamic>.from(data);
      if (eventName == 'queue_added' ||
          eventName == 'queue_removed' ||
          eventName == 'queue_updated' ||
          eventName == 'queue_items_updated') {
        queue = PlayerQueue.fromJson(rawData);
      }
    }

    _queueEventController.add(
      MusicAssistantQueueEvent(
        event: eventName,
        queueId: queueId,
        queue: queue,
        rawData: rawData,
        receivedAt: DateTime.now().toUtc(),
      ),
    );
  }

  void _handleSocketDone() {
    _connected = false;
    if (_authCompleter != null && !_authCompleter!.isCompleted) {
      _authCompleter!.completeError(StateError('WebSocket-Verbindung geschlossen.'));
    }
    _authCompleter = null;
    _authMessageId = null;
  }

  void _handleSocketError(Object error, StackTrace stackTrace) {
    _connected = false;
    if (_authCompleter != null && !_authCompleter!.isCompleted) {
      _authCompleter!.completeError(error, stackTrace);
    }
    _authCompleter = null;
    _authMessageId = null;
  }

  Uri _buildWebSocketUri(String sourceBaseUrl) {
    final normalized = sourceBaseUrl.trim();
    if (normalized.isEmpty) {
      throw StateError('MusicAssistantEventHub: baseUrl ist leer.');
    }

    final base = Uri.parse(normalized);
    final String normalizedPath = base.path.trim().replaceFirst(RegExp(r'/+$'), '');
    final path = normalizedPath.isEmpty ? '/ws' : '$normalizedPath/ws';

    return base.replace(
      scheme: base.scheme.toLowerCase() == 'https' ? 'wss' : 'ws',
      path: path,
      query: null,
      fragment: null,
    );
  }
}
