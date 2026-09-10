import 'package:flutter/foundation.dart';

enum AppLogLevel {
  trace,
  debug,
  info,
  warn,
  error,

  ;

  static AppLogLevel parse(String value) {
    return switch (value.trim().toLowerCase()) {
      'trace' => AppLogLevel.trace,
      'debug' => AppLogLevel.debug,
      'info' => AppLogLevel.info,
      'warn' || 'warning' => AppLogLevel.warn,
      'error' => AppLogLevel.error,
      _ => AppLogLevel.info,
    };
  }
}

extension on AppLogLevel {
  bool allows(AppLogLevel eventLevel) => eventLevel.index >= index;
}

class AppLogEvent {
  const AppLogEvent({
    required this.timestamp,
    required this.level,
    required this.category,
    required this.message,
    this.error,
    this.stackTrace,
  });

  final DateTime timestamp;
  final AppLogLevel level;
  final String category;
  final String message;
  final Object? error;
  final StackTrace? stackTrace;
}

abstract class AppLogSink {
  void log(AppLogEvent event);
}

class ConsoleLogSink implements AppLogSink {
  @override
  void log(AppLogEvent event) {
    debugPrint(_formatEvent(event));
  }

  String _formatEvent(AppLogEvent event) {
    final DateTime timestamp = event.timestamp;
    final String formattedTimestamp =
        '${timestamp.year.toString().padLeft(4, '0')}-'
        '${timestamp.month.toString().padLeft(2, '0')}-'
        '${timestamp.day.toString().padLeft(2, '0')} '
        '${timestamp.hour.toString().padLeft(2, '0')}:'
        '${timestamp.minute.toString().padLeft(2, '0')}:'
        '${timestamp.second.toString().padLeft(2, '0')}.'
        '${timestamp.millisecond.toString().padLeft(3, '0')}';
    final String error = event.error == null
      ? ''
      : ' | ${event.error.toString().replaceAll('\n', ' ')}';
    final String stackTrace = event.stackTrace == null
      ? ''
      : ' | ${event.stackTrace.toString().replaceAll('\n', ' ')}';
    return '[$formattedTimestamp] [${_toShortLevel(event.level)}] '
      '${event.category}: ${event.message.replaceAll('\n', ' ')}'
      '$error$stackTrace';
  }

  String _toShortLevel(AppLogLevel level) {
    return switch (level) {
      AppLogLevel.trace => 'TRC',
      AppLogLevel.debug => 'DBG',
      AppLogLevel.info => 'INF',
      AppLogLevel.warn => 'WRN',
      AppLogLevel.error => 'ERR',
    };
  }

}

class AppLogger {
  AppLogger(this.category);

  final String category;

  static final List<AppLogSink> _sinks = <AppLogSink>[
    ConsoleLogSink(),
  ];
  static AppLogLevel _minimumLevel = AppLogLevel.info;

  static void configureLevel(String value) {
    _minimumLevel = AppLogLevel.parse(value);
  }

  static void configureSinks(List<AppLogSink> sinks) {
    _sinks
      ..clear()
      ..addAll(sinks);
  }

  void debug(String message, {Object? error, StackTrace? stackTrace}) {
    _emit(
      AppLogLevel.debug,
      message,
      error: error,
      stackTrace: stackTrace,
    );
  }

  void trace(String message, {Object? error, StackTrace? stackTrace}) {
    _emit(
      AppLogLevel.trace,
      message,
      error: error,
      stackTrace: stackTrace,
    );
  }

  void info(String message, {Object? error, StackTrace? stackTrace}) {
    _emit(
      AppLogLevel.info,
      message,
      error: error,
      stackTrace: stackTrace,
    );
  }

  void warn(String message, {Object? error, StackTrace? stackTrace}) {
    _emit(
      AppLogLevel.warn,
      message,
      error: error,
      stackTrace: stackTrace,
    );
  }

  void error(String message, {Object? error, StackTrace? stackTrace}) {
    _emit(
      AppLogLevel.error,
      message,
      error: error,
      stackTrace: stackTrace,
    );
  }

  void _emit(
    AppLogLevel level,
    String message, {
    Object? error,
    StackTrace? stackTrace,
  }) {
    if (!_minimumLevel.allows(level)) {
      return;
    }

    final event = AppLogEvent(
      timestamp: DateTime.now(),
      level: level,
      category: category,
      message: message,
      error: error,
      stackTrace: stackTrace,
    );

    for (final sink in _sinks) {
      sink.log(event);
    }
  }
}
