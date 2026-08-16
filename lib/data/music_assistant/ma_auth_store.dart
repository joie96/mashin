import 'package:shared_preferences/shared_preferences.dart';

import '../../app/config/app_config.dart';

class AuthSession {
  final String baseUrl;
  final String sendspinUrl;
  final String username;
  final String token;

  const AuthSession({
    required this.baseUrl,
    required this.sendspinUrl,
    required this.username,
    required this.token,
  });
}

class AuthStore {
  static const String _baseUrlKey = 'ma.base_url';
  static const String _sendspinUrlKey = 'ma.sendspin_url';
  static const String _usernameKey = 'ma.username';
  static const String _tokenKey = 'ma.token';

  Future<AuthSession?> read() async {
    final prefs = await SharedPreferences.getInstance();
    final baseUrl = prefs.getString(_baseUrlKey)?.trim();
    final username = prefs.getString(_usernameKey)?.trim();
    final token = prefs.getString(_tokenKey)?.trim();

    if (baseUrl == null || baseUrl.isEmpty || token == null || token.isEmpty) {
      return null;
    }

    final sendspinUrl = await AppConfig.getSendspinUrl();

    return AuthSession(
      baseUrl: baseUrl,
      sendspinUrl: sendspinUrl,
      username: (username == null || username.isEmpty) ? 'user' : username,
      token: token,
    );
  }

  Future<void> write(AuthSession session) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_baseUrlKey, session.baseUrl.trim());
    await prefs.setString(_sendspinUrlKey, session.sendspinUrl.trim());
    await prefs.setString(_usernameKey, session.username.trim());
    await prefs.setString(_tokenKey, session.token.trim());
  }

  Future<void> writeSendspinUrl({
    required String baseUrl,
    String? sendspinUrl,
  }) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_baseUrlKey, baseUrl.trim());

    final configured = sendspinUrl?.trim() ?? '';
    if (configured.isNotEmpty) {
      await prefs.setString(_sendspinUrlKey, configured);
      return;
    }

    await prefs.remove(_sendspinUrlKey);
    final resolved = await AppConfig.getSendspinUrl();
    await prefs.setString(_sendspinUrlKey, resolved.trim());
  }

  Future<String> readSendspinUrl({required String baseUrl}) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_baseUrlKey, baseUrl.trim());
    final resolved = await AppConfig.getSendspinUrl();
    return resolved;
  }

  Future<void> clear() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_baseUrlKey);
    await prefs.remove(_sendspinUrlKey);
    await prefs.remove(_usernameKey);
    await prefs.remove(_tokenKey);
  }
}
