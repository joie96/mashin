import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/navigation/app_router.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/shared/detail_page_lifecycle.dart';

void main() {
  testWidgets('covered detail route pauses and resumes when popped back', (
    tester,
  ) async {
    final visibility = <bool>[];
    final navigator = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      MaterialApp(
        navigatorKey: navigator,
        navigatorObservers: [detailPageRouteObserver],
        home: _DetailLifecycleProbe(onVisibility: visibility.add),
      ),
    );
    expect(visibility, [true]);
    unawaited(
      navigator.currentState!.push(
        MaterialPageRoute<void>(
          builder: (_) => const Scaffold(body: Text('next page')),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(visibility, [true, false]);
    navigator.currentState!.pop();
    await tester.pumpAndSettle();
    expect(visibility, [true, false, true]);
  });

  test('navigation logs push and pop at info without query parameters', () {
    final entries = <(LogLevelPreference, String, String)>[];
    final observer = ShellNavigationObserver()
      ..log = (level, category, message) =>
          entries.add((level, category, message));
    final home = MaterialPageRoute<void>(
      settings: const RouteSettings(name: '/home'),
      builder: (_) => const SizedBox(),
    );
    final artist = MaterialPageRoute<void>(
      settings: const RouteSettings(name: '/artist/jellyfin/42?token=secret'),
      builder: (_) => const SizedBox(),
    );
    addTearDown(observer.dispose);
    addTearDown(home.dispose);
    addTearDown(artist.dispose);
    observer.didPush(artist, home);
    observer.didPop(artist, home);
    observer.didRemove(artist, home);
    expect(entries[0], (
      LogLevelPreference.info,
      'navigation',
      'action=push from=/home to=/artist/jellyfin/42',
    ));
    expect(entries[1], (
      LogLevelPreference.info,
      'navigation',
      'action=pop from=/artist/jellyfin/42 to=/home',
    ));
    expect(entries[2].$1, LogLevelPreference.debug);
    expect(entries.any((entry) => entry.$3.contains('secret')), isFalse);
  });
}

class _DetailLifecycleProbe extends StatefulWidget {
  const _DetailLifecycleProbe({required this.onVisibility});

  final ValueChanged<bool> onVisibility;

  @override
  State<_DetailLifecycleProbe> createState() => _DetailLifecycleProbeState();
}

class _DetailLifecycleProbeState extends State<_DetailLifecycleProbe>
    with DetailPageLifecycle<_DetailLifecycleProbe> {
  @override
  void onDetailPageVisibilityChanged(bool isVisible) =>
      widget.onVisibility(isVisible);

  @override
  Widget build(BuildContext context) =>
      const Scaffold(body: Text('detail page'));
}
