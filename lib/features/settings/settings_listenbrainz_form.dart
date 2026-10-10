import 'package:flutter/material.dart';

import '../core/core_models.dart';

class SettingsListenBrainzForm extends StatefulWidget {
  const SettingsListenBrainzForm({
    super.key,
    required this.credentials,
    required this.onChanged,
    required this.onTest,
    this.rowBuilder,
    this.fieldDecoration,
    this.isEnabled = true,
  });

  final ListenBrainzCredentials credentials;
  final ValueChanged<ListenBrainzCredentials> onChanged;
  final Future<void> Function(ListenBrainzCredentials) onTest;
  final Widget Function(String, Widget)? rowBuilder;
  final InputDecoration Function(IconData)? fieldDecoration;
  final bool isEnabled;

  @override
  State<SettingsListenBrainzForm> createState() =>
      _SettingsListenBrainzFormState();
}

class _SettingsListenBrainzFormState extends State<SettingsListenBrainzForm> {
  final _formKey = GlobalKey<FormState>();
  late final TextEditingController _usernameController;
  late final TextEditingController _tokenController;
  bool _isTesting = false;
  bool _isTokenVisible = false;
  bool _hasTested = false;
  String? _errorMessage;
  int _testGeneration = 0;

  // Construction and controller lifecycle.
  @override
  void initState() {
    super.initState();
    _usernameController = TextEditingController(
      text: widget.credentials.username,
    );
    _tokenController = TextEditingController(text: widget.credentials.token);
  }

  @override
  void didUpdateWidget(covariant SettingsListenBrainzForm oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_usernameController.text.trim() != widget.credentials.username ||
        _tokenController.text.trim() != widget.credentials.token) {
      _usernameController.text = widget.credentials.username;
      _tokenController.text = widget.credentials.token;
      _hasTested = false;
      _errorMessage = null;
      _testGeneration++;
    }
  }

  @override
  void dispose() {
    _usernameController.dispose();
    _tokenController.dispose();
    super.dispose();
  }

  // Credential commands and validation.
  Future<void> _test() async {
    if (!_formKey.currentState!.validate()) {
      return;
    }
    final generation = ++_testGeneration;
    setState(() {
      _isTesting = true;
      _hasTested = false;
      _errorMessage = null;
    });
    try {
      await widget.onTest(
        ListenBrainzCredentials(
          username: _usernameController.text.trim(),
          token: _tokenController.text.trim(),
        ),
      );
      if (mounted && generation == _testGeneration) {
        setState(() => _hasTested = true);
      }
    } catch (_) {
      if (mounted && generation == _testGeneration) {
        setState(
          () => _errorMessage =
              'Verbindungstest fehlgeschlagen. Benutzername, Token und Verbindung pruefen.',
        );
      }
    } finally {
      if (mounted) {
        setState(() => _isTesting = false);
      }
    }
  }

  String? _validateUsername(String? value) =>
      (value ?? '').trim().isEmpty ? 'Benutzername erforderlich' : null;

  String? _validateToken(String? value) =>
      (value ?? '').trim().isEmpty ? 'Token erforderlich' : null;

  void _changeCredentials(String _) {
    setState(() {
      _hasTested = false;
      _errorMessage = null;
    });
    widget.onChanged(
      ListenBrainzCredentials(
        username: _usernameController.text.trim(),
        token: _tokenController.text.trim(),
      ),
    );
  }

  Widget _row(String label, Widget control) =>
      widget.rowBuilder?.call(label, control) ?? control;

  // Settings presentation.
  @override
  Widget build(BuildContext context) {
    return Form(
      key: _formKey,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _row(
            'Benutzername',
            TextFormField(
              key: const Key('listenbrainz-username'),
              controller: _usernameController,
              enabled: widget.isEnabled && !_isTesting,
              autocorrect: false,
              enableSuggestions: false,
              validator: _validateUsername,
              onChanged: _changeCredentials,
              decoration:
                  widget.fieldDecoration?.call(Icons.person_outline) ??
                  const InputDecoration(
                    labelText: 'Benutzername',
                    prefixIcon: Icon(Icons.person_outline),
                  ),
            ),
          ),
          const SizedBox(height: 12),
          _row(
            'Token',
            TextFormField(
              key: const Key('listenbrainz-token'),
              controller: _tokenController,
              enabled: widget.isEnabled && !_isTesting,
              obscureText: !_isTokenVisible,
              autocorrect: false,
              enableSuggestions: false,
              validator: _validateToken,
              onChanged: _changeCredentials,
              onFieldSubmitted: _isTesting || !widget.isEnabled
                  ? null
                  : (_) => _test(),
              decoration:
                  (widget.fieldDecoration?.call(Icons.key_outlined) ??
                          const InputDecoration(
                            labelText: 'Token',
                            prefixIcon: Icon(Icons.key_outlined),
                          ))
                      .copyWith(
                        suffixIcon: IconButton(
                          tooltip: _isTokenVisible
                              ? 'Token verbergen'
                              : 'Token anzeigen',
                          onPressed: () => setState(
                            () => _isTokenVisible = !_isTokenVisible,
                          ),
                          icon: Icon(
                            _isTokenVisible
                                ? Icons.visibility_off_outlined
                                : Icons.visibility_outlined,
                          ),
                        ),
                      ),
            ),
          ),
          const SizedBox(height: 8),
          _row(
            'Verbindung',
            Row(
              children: <Widget>[
                OutlinedButton.icon(
                  onPressed: _isTesting || !widget.isEnabled ? null : _test,
                  label: const Text('Test'),
                  icon: _isTesting
                      ? const SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : Icon(
                          _hasTested ? Icons.check : Icons.network_check,
                          color: _hasTested ? Colors.green : null,
                        ),
                ),
              ],
            ),
          ),
          if (_errorMessage != null)
            Text(
              _errorMessage!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
        ],
      ),
    );
  }
}
