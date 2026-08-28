import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_models.dart';

Future<String?> showPlaylistNameDialog({
  required BuildContext context,
  Playlist? playlist,
}) {
  return showDialog<String>(
    context: context,
    builder: (BuildContext dialogContext) => _PlaylistNameDialog(
      initialName: playlist?.name ?? '',
      isUpdate: playlist != null,
    ),
  );
}

Future<bool> showDeletePlaylistDialog({
  required BuildContext context,
  required String playlistName,
}) async {
  final bool? confirmed = await showDialog<bool>(
    context: context,
    builder: (BuildContext dialogContext) => AlertDialog(
      title: const Text('Wiedergabeliste löschen'),
      content: Text('Möchtest du „$playlistName“ wirklich löschen?'),
      actions: <Widget>[
        TextButton(
          onPressed: () => Navigator.of(dialogContext).pop(false),
          child: const Text('Abbrechen'),
        ),
        FilledButton(
          onPressed: () => Navigator.of(dialogContext).pop(true),
          child: const Text('Löschen'),
        ),
      ],
    ),
  );
  return confirmed ?? false;
}

class _PlaylistNameDialog extends StatefulWidget {
  const _PlaylistNameDialog({
    required this.initialName,
    required this.isUpdate,
  });

  final String initialName;
  final bool isUpdate;

  @override
  State<_PlaylistNameDialog> createState() => _PlaylistNameDialogState();
}

class _PlaylistNameDialogState extends State<_PlaylistNameDialog> {
  late final TextEditingController _nameController = TextEditingController(
    text: widget.initialName,
  );

  bool get _hasName => _nameController.text.trim().isNotEmpty;

  @override
  void dispose() {
    _nameController.dispose();
    super.dispose();
  }

  void _submit() {
    final String name = _nameController.text.trim();
    if (name.isEmpty) {
      return;
    }
    Navigator.of(context).pop(name);
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final String title = widget.isUpdate
        ? 'Wiedergabeliste umbenennen'
        : 'Wiedergabeliste erstellen';

    return AlertDialog(
      title: Text(title),
      content: SizedBox(
        width: 352,
        child: TextField(
          controller: _nameController,
          autofocus: true,
          textInputAction: TextInputAction.done,
          decoration: const InputDecoration(
            labelText: 'Name',
            hintText: 'Name eingeben',
          ),
          onChanged: (_) => setState(() {}),
          onSubmitted: (_) => _submit(),
        ),
      ),
      actions: <Widget>[
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Abbrechen'),
        ),
        FilledButton(
          onPressed: _hasName ? _submit : null,
          style: FilledButton.styleFrom(backgroundColor: colors.primary),
          child: Text(widget.isUpdate ? 'Umbenennen' : 'Erstellen'),
        ),
      ],
    );
  }
}
