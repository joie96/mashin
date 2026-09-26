import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/shared/widgets/media_row_section_mobile.dart';

class _UnusedCoreBridge implements MashinCoreBridgeApi {
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();

  Widget section<T>({
    required List<T> items,
    bool isLoading = false,
    bool showAlbumArtist = true,
    bool showPlaylistDetails = true,
    double textScale = 1,
  }) {
    return MaterialApp(
      theme: AppTheme.dark(),
      home: Scaffold(
        body: MediaQuery(
          data: MediaQueryData(textScaler: TextScaler.linear(textScale)),
          child: SingleChildScrollView(
            child: MediaRowSectionMobile<T>(
              title: 'Medien',
              isLoading: isLoading,
              items: items,
              emptyText: 'Leer',
              coreBridge: coreBridge,
              showAlbumArtist: showAlbumArtist,
              showPlaylistDetails: showPlaylistDetails,
            ),
          ),
        ),
      ),
    );
  }

  testWidgets('mobile section heading is larger and not bold', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(section<Album>(items: <Album>[]));

    final Text heading = tester.widget<Text>(find.text('Medien'));
    expect(heading.style?.fontSize, 20);
    expect(heading.style?.fontWeight, FontWeight.normal);
  });

  testWidgets('mobile row title uses normal font weight', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      section<Artist>(
        items: <Artist>[
          Artist(itemId: 'artist', provider: 'test', name: 'Artist name'),
        ],
      ),
    );

    final Text title = tester.widget<Text>(find.text('Artist name'));
    expect(title.style?.fontWeight, FontWeight.normal);
  });

  testWidgets('row ends at the last line for each media type', (
    WidgetTester tester,
  ) async {
    final Artist artist = Artist(
      itemId: 'artist',
      provider: 'test',
      name: 'Artist name',
    );
    final Playlist playlist = Playlist(
      itemId: 'playlist',
      provider: 'test',
      name: 'Playlist name',
      isEditable: false,
      owner: 'Playlist owner',
    );
    final Track track = Track(
      itemId: 'track',
      provider: 'test',
      name: 'Track name',
      duration: 120,
      artists: const <Artist>[],
    );
    final Album album = Album(
      itemId: 'album',
      provider: 'test',
      name: 'Album name',
      artists: const <Artist>[],
      year: 2026,
    );

    for (final double textScale in <double>[1, 2]) {
      final List<(Widget, String)> cases = <(Widget, String)>[
        (
          section<Artist>(items: <Artist>[artist], textScale: textScale),
          'Artist name',
        ),
        (
          section<Playlist>(
            items: <Playlist>[playlist],
            showPlaylistDetails: false,
            textScale: textScale,
          ),
          'Playlist name',
        ),
        (
          section<Playlist>(items: <Playlist>[playlist], textScale: textScale),
          'Playlist owner',
        ),
        (section<Track>(items: <Track>[track], textScale: textScale), 'Track'),
        (section<Album>(items: <Album>[album], textScale: textScale), '2026'),
      ];

      for (final (Widget widget, String lastLine) in cases) {
        await tester.pumpWidget(widget);
        expect(tester.takeException(), isNull);
        final double sectionBottom = tester
            .getBottomLeft(find.byType(ListView))
            .dy;
        final double textBottom = tester.getBottomLeft(find.text(lastLine)).dy;
        expect(sectionBottom, closeTo(textBottom, 1));
      }
    }
  });

  testWidgets('playlist skeleton reserves only its visible lines', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      section<Playlist>(items: <Playlist>[], isLoading: true),
    );
    expect(tester.takeException(), isNull);
    expect(tester.getSize(find.byType(ListView)).height, 180);

    await tester.pumpWidget(
      section<Playlist>(
        items: <Playlist>[],
        isLoading: true,
        showPlaylistDetails: false,
      ),
    );
    expect(tester.takeException(), isNull);
    expect(tester.getSize(find.byType(ListView)).height, 164);
  });

  testWidgets('album artist is hidden only when requested', (
    WidgetTester tester,
  ) async {
    final Album album = Album(
      itemId: 'album',
      provider: 'test',
      name: 'Album name',
      artists: <Artist>[
        Artist.fromJson(<String, dynamic>{
          'item_id': 'artist',
          'provider': 'test',
          'name': 'Artist name',
        }),
      ],
      year: 2026,
    );

    await tester.pumpWidget(section<Album>(items: <Album>[album]));
    expect(find.text('Artist name'), findsOneWidget);

    await tester.pumpWidget(
      section<Album>(items: <Album>[album], showAlbumArtist: false),
    );
    expect(tester.takeException(), isNull);
    expect(find.text('Artist name'), findsNothing);
    expect(find.text('Album name'), findsOneWidget);
    expect(find.text('2026'), findsOneWidget);
    expect(
      tester.getBottomLeft(find.byType(ListView)).dy,
      closeTo(tester.getBottomLeft(find.text('2026')).dy, 1),
    );

    await tester.pumpWidget(
      section<Album>(items: <Album>[], isLoading: true, showAlbumArtist: false),
    );
    expect(tester.takeException(), isNull);
    expect(tester.getSize(find.byType(ListView)).height, 180);
  });
}
