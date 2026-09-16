import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_models.dart';

Map<String, dynamic> _mediaJson(String mediaType) => <String, dynamic>{
  'media_type': mediaType,
  'item_id': '$mediaType-id',
  'provider': 'library',
  'name': mediaType,
  'version': 'deluxe',
  'external_ids': <List<String>>[
    <String>['isrc', 'US-TEST-01'],
  ],
  'is_playable': false,
  'position': 4,
  'date_added': '2026-09-14T12:00:00Z',
  'provider_mappings': <Map<String, dynamic>>[
    <String, dynamic>{
      'item_id': 'source-id',
      'provider_domain': 'source',
      'provider_instance': 'source--one',
      'in_library': true,
      'is_unique': false,
      'audio_format': <String, dynamic>{
        'content_type': 'flac',
        'codec_type': 'pcm',
        'sample_rate': 96000,
        'bit_depth': 24,
        'channels': 2,
        'output_format_str': 'flac',
        'bit_rate': 1200,
      },
      'details': 'lossless',
    },
  ],
  'metadata': <String, dynamic>{
    'description_language': 'de',
    'explicit': true,
    'lyrics': 'lyrics',
    'transcript': 'transcript',
  },
};

void main() {
  tearDown(() => MediaItemImage.configureProxyBaseUrl(null));

  test('decodes all concrete Music Assistant media variants', () {
    expect(mediaItemFromJson(_mediaJson('artist')), isA<Artist>());
    expect(mediaItemFromJson(_mediaJson('album')), isA<Album>());
    expect(mediaItemFromJson(_mediaJson('track')), isA<Track>());
    expect(mediaItemFromJson(_mediaJson('playlist')), isA<Playlist>());
    expect(mediaItemFromJson(_mediaJson('radio')), isA<Radio>());
    expect(mediaItemFromJson(_mediaJson('audiobook')), isA<Audiobook>());
    expect(mediaItemFromJson(_mediaJson('podcast')), isA<Podcast>());

    final episode = _mediaJson('podcast_episode')
      ..['podcast'] = _mediaJson('podcast');
    expect(mediaItemFromJson(episode), isA<PodcastEpisode>());

    for (final mediaType in <String>[
      'folder',
      'collection',
      'announcement',
      'flow_stream',
      'plugin_source',
      'audio_source',
      'sound_effect',
      'genre',
    ]) {
      expect(mediaItemFromJson(_mediaJson(mediaType)), isA<GenericMediaItem>());
    }

    expect(
      () => mediaItemFromJson(_mediaJson('future_media_type')),
      throwsFormatException,
    );
  });

  test('decodes media fields, audio formats, and item mapping references', () {
    final track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'artists': <Map<String, dynamic>>[
        <String, dynamic>{
          'item_id': 'artist-id',
          'provider': 'library',
          'name': 'Artist',
          'media_type': 'artist',
          'available': true,
        },
      ],
      'album': <String, dynamic>{
        'item_id': 'album-id',
        'provider': 'library',
        'name': 'Album',
        'media_type': 'album',
        'image': <String, dynamic>{
          'type': 'thumb',
          'path': 'cover.jpg',
          'provider': 'library',
        },
      },
    });

    expect(track.version, 'deluxe');
    expect(track.externalIds, <List<String>>[
      <String>['isrc', 'US-TEST-01'],
    ]);
    expect(track.isPlayable, isFalse);
    expect(track.metadata?.descriptionLanguage, 'de');
    expect(track.metadata?.explicit, isTrue);
    expect(
      track.providerMappings.single.audioFormat?.contentType,
      ContentType.flac,
    );
    expect(track.providerMappings.single.inLibrary, isTrue);
    expect(track.artists.single.isItemMapping, isTrue);
    expect(track.album?.isItemMapping, isTrue);
    expect(track.album?.primaryImage?.path, 'cover.jpg');
  });

  test('resolves sized proxy URLs for playlist and album artwork', () {
    MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095/');
    final Map<String, dynamic> image = <String, dynamic>{
      'type': 'thumb',
      'path': 'relative-cover.jpg',
      'provider': 'library',
      'proxy_id': 'cover-proxy-id',
    };
    final playlist = Playlist.fromJson(<String, dynamic>{
      ..._mediaJson('playlist'),
      'metadata': <String, dynamic>{
        'images': <Map<String, dynamic>>[image],
        'cache_checksum': 'playlist checksum',
      },
    });
    final track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'album': <String, dynamic>{
        'item_id': 'album-id',
        'provider': 'library',
        'name': 'Album',
        'media_type': 'album',
        'image': image,
      },
    });

    expect(
      playlist.imageUriForSize(175),
      'http://music-assistant.local:8095/imageproxy/cover-proxy-id'
      '?checksum=playlist+checksum&size=256',
    );
    expect(
      track.imageUriForSize(256),
      'http://music-assistant.local:8095/imageproxy/cover-proxy-id'
      '?size=256',
    );

    final recentTrack = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'https://cdn.example.test/cover.jpg',
        'provider': 'deezer--instance',
        'remotely_accessible': true,
        'proxy_id': 'recent-track-proxy-id',
      },
    });

    expect(
      recentTrack.imageUriForSize(175),
      'http://music-assistant.local:8095/imageproxy/recent-track-proxy-id'
      '?size=256',
    );
  });

  test('encodes direct track and reference images for typed Rust commands', () {
    final Track track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'track.jpg',
        'provider': 'library',
        'proxy_id': 'track-image',
      },
      'artists': <Map<String, dynamic>>[
        <String, dynamic>{
          'media_type': 'artist',
          'item_id': 'artist-id',
          'provider': 'library',
          'name': 'Artist',
          'image': <String, dynamic>{
            'type': 'thumb',
            'path': 'artist.jpg',
            'provider': 'library',
            'proxy_id': 'artist-image',
          },
        },
      ],
      'album': <String, dynamic>{
        'media_type': 'album',
        'item_id': 'album-id',
        'provider': 'library',
        'name': 'Album',
        'image': <String, dynamic>{
          'type': 'thumb',
          'path': 'album.jpg',
          'provider': 'library',
          'proxy_id': 'album-image',
        },
      },
    });

    final Map<String, dynamic> payload = createMusicAssistantMediaItemPayload(
      track,
    );

    expect(payload['image'], <String, dynamic>{
      'type': 'thumb',
      'path': 'track.jpg',
      'provider': 'library',
      'remotely_accessible': false,
      'proxy_id': 'track-image',
    });
    expect(payload['artists'][0]['image']['proxy_id'], 'artist-image');
    expect(payload['album']['image']['proxy_id'], 'album-image');
  });

  test('decodes canonical Music Assistant player and session contracts', () {
    final player = MusicAssistantPlayer.fromJson(<String, dynamic>{
      'player_id': 'living-room',
      'provider': 'cast',
      'type': 'player',
      'name': 'Living Room',
      'available': true,
      'device_info': <String, dynamic>{
        'model': 'Speaker',
        'manufacturer': 'Example',
        'software_version': '1.2.3',
      },
      'playback_state': 'playing',
      'current_media': <String, dynamic>{
        'uri': 'library://track/1',
        'media_type': 'track',
        'queue_item_id': 'queue-1',
      },
      'output_protocols': <Map<String, dynamic>>[
        <String, dynamic>{
          'output_protocol_id': 'http',
          'name': 'HTTP',
          'protocol_domain': 'builtin',
        },
      ],
    });
    final user = MusicAssistantUser.fromJson(<String, dynamic>{
      'user_id': 'user-1',
      'username': 'Ada',
      'role': 'admin',
      'enabled': true,
      'provider_filter': <String>['library'],
      'player_filter': <String>['living-room'],
    });
    final session = MusicAssistantSession.fromJson(<String, dynamic>{
      'server_url': 'https://music-assistant.example',
      'access_token': 'token',
      'user_id': 'user-1',
      'username': 'Ada',
    });

    expect(player.deviceInfo.softwareVersion, '1.2.3');
    expect(player.playbackState, PlayState.playing);
    expect(player.currentMedia?.queueItemId, 'queue-1');
    expect(player.outputProtocols.single.protocolDomain, 'builtin');
    expect(user.providerFilter, <String>['library']);
    expect(session.accessToken, 'token');
  });
}
