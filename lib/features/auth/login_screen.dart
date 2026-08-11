import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';

typedef LoginSubmit = Future<String?> Function({
  required String baseUrl,
  required String username,
  required String password,
});

class LoginScreen extends StatefulWidget {
  final String initialBaseUrl;
  final String? initialUsername;
  final LoginSubmit onSubmit;

  const LoginScreen({
    super.key,
    required this.initialBaseUrl,
    required this.onSubmit,
    this.initialUsername,
  });

  @override
  State<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends State<LoginScreen> {
  final GlobalKey<FormState> _formKey = GlobalKey<FormState>();
  late final TextEditingController _serverController;
  late final TextEditingController _usernameController;
  late final TextEditingController _passwordController;

  bool _isSubmitting = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _serverController = TextEditingController(text: widget.initialBaseUrl);
    _usernameController = TextEditingController(text: widget.initialUsername ?? '');
    _passwordController = TextEditingController();
  }

  @override
  void dispose() {
    _serverController.dispose();
    _usernameController.dispose();
    _passwordController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 460),
        child: Card(
          margin: const EdgeInsets.all(24),
          child: Padding(
            padding: const EdgeInsets.all(22),
            child: Form(
              key: _formKey,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  Text(
                    'Bei Music Assistant anmelden',
                    style: Theme.of(context).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w700),
                  ),
                  const SizedBox(height: 16),
                  TextFormField(
                    controller: _serverController,
                    keyboardType: TextInputType.url,
                    decoration: const InputDecoration(
                      labelText: 'Server URL',
                      hintText: 'http://192.168.1.2:8095',
                    ),
                    validator: (String? value) {
                      final text = value?.trim() ?? '';
                      if (text.isEmpty) {
                        return 'Bitte Server URL eingeben';
                      }

                      final uri = Uri.tryParse(text);
                      if (uri == null || (!uri.hasScheme || !uri.hasAuthority)) {
                        return 'Bitte eine gueltige URL eingeben';
                      }

                      return null;
                    },
                  ),
                  const SizedBox(height: 12),
                  TextFormField(
                    controller: _usernameController,
                    decoration: const InputDecoration(
                      labelText: 'Benutzername',
                    ),
                    validator: (String? value) {
                      if ((value?.trim() ?? '').isEmpty) {
                        return 'Bitte Benutzernamen eingeben';
                      }

                      return null;
                    },
                  ),
                  const SizedBox(height: 12),
                  TextFormField(
                    controller: _passwordController,
                    obscureText: true,
                    decoration: const InputDecoration(
                      labelText: 'Passwort',
                    ),
                    validator: (String? value) {
                      if ((value ?? '').isEmpty) {
                        return 'Bitte Passwort eingeben';
                      }

                      return null;
                    },
                    onFieldSubmitted: (_) => _submit(),
                  ),
                  if (_error != null) ...<Widget>[
                    const SizedBox(height: 12),
                    Text(
                      _error!,
                      style: TextStyle(color: Theme.of(context).colorScheme.error),
                    ),
                  ],
                  const SizedBox(height: 18),
                  FilledButton.icon(
                    onPressed: _isSubmitting ? null : _submit,
                    icon: _isSubmitting
                        ? SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: colors.bgPrimary,
                            ),
                          )
                        : const Icon(Icons.login_rounded),
                    label: Text(_isSubmitting ? 'Anmeldung...' : 'Anmelden'),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _submit() async {
    if (_isSubmitting) {
      return;
    }

    final formState = _formKey.currentState;
    if (formState == null || !formState.validate()) {
      return;
    }

    setState(() {
      _isSubmitting = true;
      _error = null;
    });

    final error = await widget.onSubmit(
      baseUrl: _serverController.text.trim(),
      username: _usernameController.text.trim(),
      password: _passwordController.text,
    );

    if (!mounted) {
      return;
    }

    setState(() {
      _isSubmitting = false;
      _error = error;
    });
  }
}
