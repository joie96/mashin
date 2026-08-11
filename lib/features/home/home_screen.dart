import 'package:flutter/material.dart';

import '../../data/music_assistant/music_assistant_client.dart';

class HomeScreen extends StatelessWidget {
  final MusicAssistantClient client;

  const HomeScreen({
    super.key,
    required this.client,
  });

  @override
  Widget build(BuildContext context) {
    return const SizedBox.expand();
  }
}
