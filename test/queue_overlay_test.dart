import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/queue/queue_overlay_desktop.dart';
import 'package:mashin_flutter/features/queue/queue_overlay_mobile.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_desktop.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_mobile.dart';

class _QueueBridge implements MashinCoreBridgeApi {
  final calls = <(String, String, int?)>[];

  // Record only the direct server mutation contract.
  @override
  Future<void> moveQueueItem(String itemId, int positionShift) async {
    calls.add(('move', itemId, positionShift));
  }

  @override
  Future<void> moveQueueItemToEnd(String itemId) async {
    calls.add(('end', itemId, null));
  }

  @override
  Future<void> deleteQueueItem(String itemId) async {
    calls.add(('delete', itemId, null));
  }

  @override
  Future<void> selectQueueItem(String itemId) async {
    calls.add(('select', itemId, null));
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  for (final isMobile in <bool>[false, true]) {
    for (final action in <String>[
      'Als N\u00e4chstes spielen',
      'Als Letztes spielen',
      'Aus Queue entfernen',
      'Abspielen',
    ]) {
      testWidgets('${isMobile ? 'mobile' : 'desktop'} queue: $action', (
        tester,
      ) async {
        tester.view.physicalSize = Size(isMobile ? 480 : 1400, 850);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _QueueBridge();
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        final items = List<QueueItem>.generate(
          4,
          (index) => QueueItem(
            queueItemId: 'queue-$index',
            name: 'Track $index',
            mediaItem: Track(
              itemId: 'track-$index',
              provider: 'test',
              name: 'Track $index',
              duration: 120,
              artists: const <Artist>[],
            ),
          ),
        );
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: isMobile
                  ? QueueOverlayMobile(
                      queueTrackCountText: '4',
                      queueTotalDurationText: '8:00',
                      queueItems: items,
                      currentQueueIndex: 0,
                      coreBridge: bridge,
                      coreEvents: events,
                      onDismissDragUpdate: (_) {},
                      onDismissDragEnd: (_) {},
                    )
                  : QueueOverlayDesktop(
                      trackTitle: 'Track 0',
                      trackSubtitle: 'Artist',
                      trackCoverUrl: null,
                      queueTrackCountText: '4',
                      queueTotalDurationText: '8:00',
                      dontStopTheMusicEnabled: false,
                      queueItems: items,
                      currentQueueIndex: 0,
                      coreBridge: bridge,
                      coreEvents: events,
                      onClose: () {},
                      onToggleDontStopTheMusic: () async {},
                    ),
            ),
          ),
        );
        await tester.pumpAndSettle();

        final targets = <QueueItem>[items[3], items[1]];
        if (isMobile) {
          final table = tester.widget<MediaTableSectionMobile<QueueItem>>(
            find.byType(MediaTableSectionMobile<QueueItem>),
          );
          table.onSelectionMenuTap!(targets);
        } else {
          final table = tester.widget<MediaTableSectionDesktop<QueueItem>>(
            find.byType(MediaTableSectionDesktop<QueueItem>),
          );
          table.onSelectionChanged!(targets);
          table.onRowContextMenu!(items[1], 1, const Offset(600, 150));
        }
        await tester.pumpAndSettle();
        await tester.tap(find.text(action));
        await tester.pumpAndSettle();

        final expected = switch (action) {
          'Als N\u00e4chstes spielen' => <(String, String, int?)>[
            ('move', 'queue-3', 0),
            ('move', 'queue-1', 0),
          ],
          'Als Letztes spielen' => <(String, String, int?)>[
            ('end', 'queue-1', null),
            ('end', 'queue-3', null),
          ],
          'Aus Queue entfernen' => <(String, String, int?)>[
            ('delete', 'queue-3', null),
            ('delete', 'queue-1', null),
          ],
          _ => <(String, String, int?)>[('select', 'queue-3', null)],
        };
        expect(bridge.calls, expected);
        expect(items.map((item) => item.queueItemId), <String>[
          'queue-0',
          'queue-1',
          'queue-2',
          'queue-3',
        ]);
        expect(tester.takeException(), isNull);
      });
    }
  }
}
