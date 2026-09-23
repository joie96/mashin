import 'package:flutter/foundation.dart';

class AppLogBuffer extends ChangeNotifier {
  AppLogBuffer._();

  static final AppLogBuffer shared = AppLogBuffer._();

  static const int _capacity = 10000;
  final List<String> _lines = <String>[];

  List<String> get lines => List<String>.unmodifiable(_lines);

  void add(String line) {
    _lines.add(line);
    if (_lines.length > _capacity) {
      _lines.removeRange(0, _lines.length - _capacity);
    }
    notifyListeners();
  }
}
