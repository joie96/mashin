// ignore_for_file: prefer_const_constructors
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/core/core_userdata.dart';

class _UnusedCoreBridge extends Fake implements MashinCoreBridgeApi {
  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();
}

class _FavoriteSnapshotBridge extends _UnusedCoreBridge {
  bool hasFavorite = true;

  @override
  Future<Map<String, dynamic>> getUserData() => loadUserData();

  @override
  Future<Map<String, dynamic>> loadUserData() async => <String, dynamic>{
    'favorites': <String, dynamic>{
      'tracks': <Map<String, dynamic>>[
        if (hasFavorite)
          _mediaJson('track')
            ..['favorite'] = true
            ..['uri'] = 'library://track/track-id',
      ],
    },
    'playlists': <Map<String, dynamic>>[],
  };
}

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
  test(
    'recommendation snapshots preserve independent folder loading and cached playlist tracks',
    () {
      final snapshot = RecommendationSnapshot.fromJson({
        'is_loading': true,
        'folders': [
          {
            'item_id': 'ready',
            'provider': 'lb',
            'items': [],
            'is_loading': false,
            'playlist_tracks': [_mediaJson('track')],
          },
          {
            'item_id': 'pending',
            'provider': 'lb',
            'items': [],
            'is_loading': true,
          },
        ],
      });
      expect(snapshot.isLoading, isTrue);
      expect(snapshot.isFolderLoading('ready'), isFalse);
      expect(snapshot.isFolderLoading('pending'), isTrue);
      expect(snapshot.isFolderLoading('not-discovered'), isTrue);
      expect(snapshot.folders.first.playlistTracks.single.itemId, 'track-id');
      expect(const RecommendationSnapshot().isFolderLoading('absent'), isFalse);
    },
  );

  test(
    'queue playback provider prefers stream details over library mappings',
    () {
      final item = QueueItem.fromJson(<String, dynamic>{
        'queue_item_id': 'queued-track',
        'media_item': _mediaJson('track'),
        'streamdetails': <String, dynamic>{'provider': 'deezer--one'},
      });

      expect(item.playbackProvider, 'deezer--one');
      expect(item.mediaItem.provider, 'library');
    },
  );

  test(
    'queue playback provider resolves library mappings without a stream',
    () {
      for (final streamDetails in <Object?>[
        null,
        <String, dynamic>{'provider': '  '},
      ]) {
        final item = QueueItem.fromJson(<String, dynamic>{
          'queue_item_id': 'queued-track',
          'media_item': _mediaJson('track'),
          'streamdetails': streamDetails,
        });

        expect(item.playbackProvider, 'source--one');
      }
    },
  );

  test(
    'queue playback provider preserves direct providers and unmapped items',
    () {
      for (final provider in <String>['deezer--one', 'library', '']) {
        final item = QueueItem.fromJson(<String, dynamic>{
          'queue_item_id': 'queued-track',
          'media_item': <String, dynamic>{
            'media_type': 'track',
            'item_id': 'track-id',
            'provider': provider,
            'name': 'Track',
          },
        });

        expect(item.playbackProvider, provider);
      }
    },
  );

  test('queue artwork survives parsing without media artwork', () {
    final item = QueueItem.fromJson(<String, dynamic>{
      'queue_item_id': 'queued-track',
      'media_item': <String, dynamic>{
        'media_type': 'track',
        'item_id': 'track-id',
        'provider': 'library',
        'name': 'Queued track',
      },
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'https://example.com/queue-cover.jpg',
        'provider': 'test',
        'remotely_accessible': true,
      },
    });

    expect(item.mediaItem.imageUriForSize(80), isNull);
    expect(item.image, isNotNull);
    expect(item.imageUriForSize(80), item.image!.getProxyPath(size: 80));
    expect(item.imageUriForSize(512), item.image!.getProxyPath(size: 512));
  });

  test('queue artwork falls back to media artwork when absent or null', () {
    for (final hasImageField in <bool>[false, true]) {
      final json = <String, dynamic>{
        'queue_item_id': 'queued-track',
        'media_item': _mediaJson('track')
          ..['metadata'] = <String, dynamic>{
            'images': <Map<String, dynamic>>[
              <String, dynamic>{
                'type': 'thumb',
                'path': 'https://example.com/media-cover.jpg',
                'provider': 'test',
                'remotely_accessible': true,
              },
            ],
          },
        if (hasImageField) 'image': null,
      };
      final item = QueueItem.fromJson(json);

      expect(item.image, isNull);
      expect(item.imageUriForSize(80), isNotNull);
      expect(item.imageUriForSize(80), item.mediaItem.imageUriForSize(80));
    }
  });

  test(
    'provider aliases without a URI use the confirmed library favorite',
    () async {
      final userData = UserData(coreBridge: _FavoriteSnapshotBridge());
      await userData.loadLibraryData();
      final providerItem = Track.fromJson(<String, dynamic>{
        'item_id': 'source-id',
        'provider': 'source--one',
        'name': 'Track',
      });
      expect(userData.isFavorite(providerItem), isTrue);
      userData.dispose();
    },
  );

  test('playlist ownership survives favorite projection', () {
    final userData = UserData(coreBridge: _UnusedCoreBridge());
    final playlist = Playlist.fromJson(
      _mediaJson('playlist')
        ..['access'] = <String, dynamic>{
          'owner': 'user-one',
          'sharing': 'private',
          'shared_users': <String>[],
          'collaborative': false,
        },
    );
    final projected = userData.withFavoriteStatus(playlist);
    expect(
      createMusicAssistantMediaItemPayload(projected)['access'],
      <String, dynamic>{
        'owner': 'user-one',
        'sharing': 'private',
        'shared_users': <String>[],
        'collaborative': false,
      },
    );
    userData.dispose();
  });

  test(
    'favorite indexes preserve identities, mappings and URI matches',
    () async {
      final bridge = _FavoriteSnapshotBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(userData.dispose);
      await userData.loadLibraryData();
      for (final json in <Map<String, dynamic>>[
        {'item_id': 'track-id', 'provider': 'library', 'name': 'Direct'},
        {
          'item_id': 'source-id',
          'provider': 'source--one',
          'name': 'Provider alias',
        },
        {
          'item_id': 'other',
          'provider': 'another',
          'name': 'Shared mapping',
          'provider_mappings': [
            {
              'item_id': 'source-id',
              'provider_instance': 'source--one',
              'provider_domain': 'source',
            },
          ],
        },
        {
          'item_id': 'other',
          'provider': 'another',
          'name': 'Primary mapping',
          'provider_mappings': [
            {
              'item_id': 'track-id',
              'provider_instance': 'library',
              'provider_domain': 'library',
            },
          ],
        },
        {
          'item_id': 'other',
          'provider': 'another',
          'name': 'URI',
          'uri': 'library://track/track-id',
        },
      ]) {
        expect(
          userData.isFavorite(Track.fromJson(json)),
          isTrue,
          reason: json['name'] as String,
        );
      }
      final favorite = userData.favoriteTracks.single;
      expect(userData.withFavoriteStatus(favorite), same(favorite));
      expect(
        userData.isFavorite(
          Album.fromJson({
            'item_id': favorite.itemId,
            'provider': favorite.provider,
            'name': 'Other type',
          }),
        ),
        isFalse,
      );
      final unrelated = Track.fromJson({
        'item_id': 'other',
        'provider': 'unrelated',
        'name': 'Unrelated',
        'favorite': true,
      });
      expect(userData.isFavorite(unrelated), isFalse);
      expect(userData.withFavoriteStatus(unrelated).favorite, isFalse);
      bridge.hasFavorite = false;
      await userData.refreshLibrarySnapshot();
      expect(userData.isFavorite(favorite), isFalse);
      expect(
        userData.isFavorite(
          Track.fromJson({
            'item_id': 'source-id',
            'provider': 'source--one',
            'name': 'Alias',
          }),
        ),
        isFalse,
      );
    },
  );

  test('favorite projection preserves complete modeled media payloads', () {
    final userData = UserData(coreBridge: _UnusedCoreBridge());
    for (final mediaType in <String>['track', 'album', 'artist', 'playlist']) {
      final json = _mediaJson(mediaType)
        ..['uri'] = 'library://$mediaType/item'
        ..['duration'] = 233
        ..['disc_number'] = 2
        ..['track_number'] = 4
        ..['year'] = 2026
        ..['artist_type'] = 'artist'
        ..['is_dynamic'] = true;
      final original = mediaItemFromJson(json);
      final projected = userData.withFavoriteStatus(original);
      expect(projected, same(original));
      expect(
        createMusicAssistantMediaItemPayload(projected),
        createMusicAssistantMediaItemPayload(original),
      );
      if (projected is Playlist) {
        expect(
          createMusicAssistantMediaItemPayload(projected)['is_dynamic'],
          isTrue,
        );
      }
    }
    userData.dispose();
  });

  test('queue availability follows MA without local cache overrides', () {
    final media = Track.fromJson(_mediaJson('track')..['available'] = false);
    final partial = QueueItem(
      queueItemId: 'partial',
      mediaItem: media,
      available: false,
    );
    final complete = QueueItem(
      queueItemId: 'complete',
      mediaItem: media,
      available: true,
    );
    expect(partial.canPlay, isFalse);
    expect(partial.isAvailable, isFalse);
    expect(complete.canPlay, isTrue);
    expect(complete.isAvailable, isTrue);
    final buffering = QueueItem(
      queueItemId: 'buffering',
      mediaItem: media,
      available: true,
    );
    expect(buffering.canPlay, isTrue);
    expect(buffering.isAvailable, isTrue);
  });

  test('availability survives parsing and playback payload serialization', () {
    for (final mediaType in <String>[
      'track',
      'album',
      'artist',
      'playlist',
      'radio',
      'audiobook',
      'podcast',
      'folder',
    ]) {
      final json = _mediaJson(mediaType)..['available'] = false;
      final item = mediaItemFromJson(json);
      expect(item.isAvailable, isFalse);
      expect(item.canPlay, isFalse);
      expect(createMusicAssistantMediaItemPayload(item)['available'], isFalse);
    }
  });

  test(
    'unknown availability is optimistic but unavailable mappings are not',
    () {
      final json = _mediaJson('track')..['is_playable'] = true;
      expect(Track.fromJson(json).canPlay, isTrue);
      (json['provider_mappings'] as List).first['available'] = false;
      expect(Track.fromJson(json).canPlay, isFalse);
      json['provider_mappings'] = null;
      expect(Track.fromJson(json).canPlay, isTrue);
    },
  );

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

  test('decodes compact nested media as complete typed models', () {
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
    expect(track.artists.single, isA<Artist>());
    expect(track.artists.single.metadata, isNull);
    expect(track.album, isA<Album>());
    expect(track.album?.year, isNull);
    expect(track.album?.primaryImage?.path, 'cover.jpg');
  });

  test('compact playlist images survive recommendation decoding', () {
    MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095');
    final Playlist playlist = Playlist.fromJson(<String, dynamic>{
      'item_id': 'artist-radio',
      'provider': 'listenbrainz_recommendations--test',
      'name': 'Radio: Artist',
      'media_type': 'playlist',
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'https://example.test/artist.jpg',
        'provider': 'url',
        'remotely_accessible': true,
        'proxy_id': 'artist-cover',
      },
    });

    expect(playlist.primaryImage?.path, 'https://example.test/artist.jpg');
    expect(
      playlist.imageUriForSize(256),
      'http://music-assistant.local:8095/imageproxy/artist-cover?size=256',
    );
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

  test(
    'YouTube metadata covers survive favorite payload and snapshot parsing',
    () {
      MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095');
      final image = <String, dynamic>{
        'type': 'thumb',
        'path': 'https://yt3.googleusercontent.com/cover',
        'provider': 'ytmusic--test',
        'remotely_accessible': true,
        'proxy_id': 'youtube-cover',
      };
      final track = Track.fromJson(<String, dynamic>{
        'media_type': 'track',
        'item_id': 'video',
        'provider': 'ytmusic--test',
        'name': 'Track',
        'uri': 'ytmusic--test://track/video',
        'image': null,
        'metadata': <String, dynamic>{
          'images': <Map<String, dynamic>>[image],
          'genres': null,
        },
      });
      final payload = createMusicAssistantMediaItemPayload(track);
      expect(payload['image'], isNull);
      expect(payload['metadata']['images'].single, image);
      final restored = Track.fromJson(<String, dynamic>{
        ...payload,
        'image': image,
        'favorite': true,
      });
      expect(restored.favorite, isTrue);
      expect(
        restored.imageUriForSize(256),
        'http://music-assistant.local:8095/imageproxy/youtube-cover?size=256',
      );
      expect(track.imageUriForSize(256), restored.imageUriForSize(256));
    },
  );

  test(
    'encodes direct track and nested model images for typed Rust commands',
    () {
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
      expect(
        payload['artists'][0]['metadata']['images'][0]['proxy_id'],
        'artist-image',
      );
      expect(payload['artists'][0]['artist_type'], isNull);
      expect(
        payload['album']['metadata']['images'][0]['proxy_id'],
        'album-image',
      );
      expect(payload['album']['year'], isNull);
    },
  );

  test('retains complete album metadata in a queued track payload', () {
    final track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'disc_number': 2,
      'track_number': 3,
      'artists': <Map<String, dynamic>>[
        <String, dynamic>{
          'item_id': 'artist-1',
          'provider': 'library',
          'name': 'Artist',
          'metadata': <String, dynamic>{'images': <Object>[]},
        },
      ],
      'album': <String, dynamic>{
        'item_id': 'album-1',
        'provider': 'library',
        'name': 'Complete album',
        'year': 2014,
        'album_type': 'album',
        'artists': <Map<String, dynamic>>[
          <String, dynamic>{
            'item_id': 'artist-1',
            'provider': 'library',
            'name': 'Artist',
            'metadata': <String, dynamic>{'images': <Object>[]},
          },
        ],
        'metadata': <String, dynamic>{'images': <Object>[]},
      },
    });

    final payload = createMusicAssistantMediaItemPayload(track);

    expect(payload['album']['year'], 2014);
    expect(payload['album']['album_type'], 'album');
    expect(payload['album']['artists'][0]['metadata'], isNotNull);
    expect(payload['artists'][0]['metadata'], isNotNull);
    expect(payload['disc_number'], 2);
    expect(payload['track_number'], 3);
  });

  test('encodes a compact podcast as a complete episode payload', () {
    final episode = PodcastEpisode.fromJson(<String, dynamic>{
      ..._mediaJson('podcast_episode'),
      'podcast': <String, dynamic>{
        'item_id': 'podcast-1',
        'provider': 'library',
        'name': 'Podcast',
        'image': <String, dynamic>{
          'type': 'thumb',
          'path': 'podcast.jpg',
          'provider': 'library',
          'proxy_id': 'podcast-proxy-id',
        },
      },
    });

    final payload = createMusicAssistantMediaItemPayload(episode);
    expect(episode.podcast, isA<Podcast>());
    expect(payload['podcast']['media_type'], 'podcast');
    expect(payload['podcast']['publisher'], isNull);
    expect(payload['podcast']['total_episodes'], isNull);
    expect(
      payload['podcast']['metadata']['images'][0]['proxy_id'],
      'podcast-proxy-id',
    );
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
