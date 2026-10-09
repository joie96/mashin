import 'package:flutter/material.dart';

import '../core/core_models.dart';

class SettingsListenBrainzForm extends StatefulWidget {
  const SettingsListenBrainzForm({
    super.key,
    required this.credentials,
    required this.onSave,
  });

  final ListenBrainzCredentials credentials;
  final Future<void> Function(ListenBrainzCredentials) onSave;

  @override
  State<SettingsListenBrainzForm> createState() =>
      _SettingsListenBrainzFormState();
}

class _SettingsListenBrainzFormState extends State<SettingsListenBrainzForm> {
  final _formKey = GlobalKey<FormState>();
  late final TextEditingController _usernameController;
  late final TextEditingController _tokenController;
  bool _isSaving = false;
  bool _isTokenVisible = false;
  bool _hasSaved = false;
  String? _errorMessage;

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
  void dispose() {
    _usernameController.dispose();
    _tokenController.dispose();
    super.dispose();
  }

  // Credential commands and validation.
  Future<void> _save() async {
    if (!_formKey.currentState!.validate()) {
      return;
    }
    setState(() {
      _isSaving = true;
      _hasSaved = false;
      _errorMessage = null;
    });
    try {
      await widget.onSave(
        ListenBrainzCredentials(
          username: _usernameController.text.trim(),
          token: _tokenController.text.trim(),
        ),
      );
      if (mounted) {
        setState(() => _hasSaved = true);
      }
    } catch (_) {
      if (mounted) {
        setState(
          () => _errorMessage =
              'Speichern fehlgeschlagen. Benutzername, Token und Verbindung pruefen.',
        );
      }
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  String? _validateUsername(String? value) =>
      (value ?? '').trim().isEmpty && _tokenController.text.trim().isNotEmpty
      ? 'Benutzername erforderlich'
      : null;

  String? _validateToken(String? value) =>
      (value ?? '').trim().isEmpty && _usernameController.text.trim().isNotEmpty
      ? 'Token erforderlich'
      : null;

  void _clearStatus(String _) => setState(() {
    _hasSaved = false;
    _errorMessage = null;
  });

  // Settings presentation.
  @override
  Widget build(BuildContext context) {
    return Form(
      key: _formKey,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          TextFormField(
            controller: _usernameController,
            enabled: !_isSaving,
            autocorrect: false,
            enableSuggestions: false,
            validator: _validateUsername,
            onChanged: _clearStatus,
            decoration: const InputDecoration(
              labelText: 'Benutzername',
              prefixIcon: Icon(Icons.person_outline),
            ),
          ),
          const SizedBox(height: 12),
          TextFormField(
            controller: _tokenController,
            enabled: !_isSaving,
            obscureText: !_isTokenVisible,
            autocorrect: false,
            enableSuggestions: false,
            validator: _validateToken,
            onChanged: _clearStatus,
            onFieldSubmitted: _isSaving ? null : (_) => _save(),
            decoration: InputDecoration(
              labelText: 'Token',
              prefixIcon: const Icon(Icons.key_outlined),
              suffixIcon: IconButton(
                tooltip: _isTokenVisible ? 'Token verbergen' : 'Token anzeigen',
                onPressed: () =>
                    setState(() => _isTokenVisible = !_isTokenVisible),
                icon: Icon(
                  _isTokenVisible
                      ? Icons.visibility_off_outlined
                      : Icons.visibility_outlined,
                ),
              ),
            ),
          ),
          const SizedBox(height: 8),
          Row(
            children: <Widget>[
              IconButton(
                tooltip: 'ListenBrainz speichern',
                onPressed: _isSaving ? null : _save,
                icon: _isSaving
                    ? const SizedBox(
                        width: 20,
                        height: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.save_outlined),
              ),
              if (_hasSaved)
                Expanded(
                  child: Text(
                    _usernameController.text.trim().isEmpty
                        ? 'Scrobbling deaktiviert'
                        : 'Scrobbling aktiviert',
                  ),
                ),
            ],
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
