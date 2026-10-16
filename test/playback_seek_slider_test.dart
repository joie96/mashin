import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/playerbar/playback_seek_slider.dart';

void main() {
  Future<void> showSlider(
    WidgetTester tester, {
    double value = 20,
    double? bufferedValue = 60,
    bool enabled = true,
    Future<void> Function(double)? onSeek,
  }) => tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: PlaybackSeekSlider(
          value: value,
          max: 100,
          bufferedValue: bufferedValue,
          enabled: enabled,
          onSeek: onSeek ?? (_) async {},
        ),
      ),
    ),
  );

  testWidgets('shows buffer ahead without moving the audible position', (
    WidgetTester tester,
  ) async {
    await showSlider(tester);
    final Slider slider = tester.widget<Slider>(find.byType(Slider));
    expect(slider.value, 20);
    expect(slider.secondaryTrackValue, 60);
  });

  testWidgets('clamps buffered end to the track duration', (
    WidgetTester tester,
  ) async {
    await showSlider(tester, bufferedValue: 150);
    expect(tester.widget<Slider>(find.byType(Slider)).secondaryTrackValue, 100);
  });

  testWidgets('does not invent buffer for remote or stale positions', (
    WidgetTester tester,
  ) async {
    for (final double? buffered in <double?>[null, 0, 10, 20, -1]) {
      await showSlider(tester, bufferedValue: buffered);
      expect(
        tester.widget<Slider>(find.byType(Slider)).secondaryTrackValue,
        null,
      );
    }
  });

  testWidgets('disabled playback neither shows buffer nor permits seeking', (
    WidgetTester tester,
  ) async {
    await showSlider(tester, enabled: false);
    final Slider slider = tester.widget<Slider>(find.byType(Slider));
    expect(slider.secondaryTrackValue, null);
    expect(slider.onChanged, null);
    expect(slider.onChangeEnd, null);
  });

  testWidgets('keeps drag preview through updates and seeks only on release', (
    WidgetTester tester,
  ) async {
    final List<double> seeks = <double>[];
    Future<void> onSeek(double seconds) async => seeks.add(seconds);
    await showSlider(tester, onSeek: onSeek);
    tester.widget<Slider>(find.byType(Slider)).onChanged!(70);
    await tester.pump();
    await showSlider(tester, value: 21, onSeek: onSeek);
    Slider slider = tester.widget<Slider>(find.byType(Slider));
    expect(slider.value, 70);
    expect(slider.secondaryTrackValue, null);
    expect(seeks, isEmpty);
    slider.onChangeEnd!(70);
    await tester.pump();
    expect(seeks, <double>[70]);
    slider = tester.widget<Slider>(find.byType(Slider));
    expect(slider.value, 21);
    expect(slider.secondaryTrackValue, 60);
  });
}
