import 'package:shared_preferences/shared_preferences.dart';

class MaAuthSession {
  final String baseUrl;
  final String username;
  final String token;

  const MaAuthSession({
    required this.baseUrl,
    required this.username,
    required this.token,
  });
}

class MaAuthStore {
  static const String _baseUrlKey = 'ma.base_url';
  static const String _usernameKey = 'ma.username';
  static const String _tokenKey = 'ma.token';

  Future<MaAuthSession?> read() async {
    final prefs = await SharedPreferences.getInstance();
    final baseUrl = prefs.getString(_baseUrlKey)?.trim();
    final username = prefs.getString(_usernameKey)?.trim();
    final token = prefs.getString(_tokenKey)?.trim();

    if (baseUrl == null || baseUrl.isEmpty || token == null || token.isEmpty) {
      return null;
    }

    return MaAuthSession(
      baseUrl: baseUrl,
      username: (username == null || username.isEmpty) ? 'user' : username,
      token: token,
    );
  }

  Future<void> write(MaAuthSession session) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_baseUrlKey, session.baseUrl.trim());
    await prefs.setString(_usernameKey, session.username.trim());
    await prefs.setString(_tokenKey, session.token.trim());
  }

  Future<void> clear() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_baseUrlKey);
    await prefs.remove(_usernameKey);
    await prefs.remove(_tokenKey);
  }
}
