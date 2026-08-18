import 'dart:developer' as developer;

enum AppLogLevel {
  debug,
  info,
  warn,
  error,
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
    developer.log(
      event.message,
      name: event.category,
      level: _toDeveloperLevel(event.level),
      time: event.timestamp,
      error: event.error,
      stackTrace: event.stackTrace,
    );
  }

  int _toDeveloperLevel(AppLogLevel level) {
    return switch (level) {
      AppLogLevel.debug => 500,
      AppLogLevel.info => 800,
      AppLogLevel.warn => 900,
      AppLogLevel.error => 1000,
    };
  }
}

class AppLogger {
  AppLogger(this.category);

  final String category;

  static final List<AppLogSink> _sinks = <AppLogSink>[
    ConsoleLogSink(),
  ];

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
