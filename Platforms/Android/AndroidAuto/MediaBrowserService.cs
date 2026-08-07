using Android.App;
using Android.Content;
using Android.OS;
using Android.Runtime;
using Android.Support.V4.Media;
using Android.Support.V4.Media.Session;
using AndroidX.Media;
using AndroidX.Media.Utils;
using Java.Util;
using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Maui;

namespace mashin.Platforms.Android.AndroidAuto;

[Service(Exported = true, Enabled = true, Name = "com.companyname.mashin.MediaBrowserService")]
[IntentFilter(new[] { "android.media.browse.MediaBrowserService" })]
public sealed class MediaBrowserService : MediaBrowserServiceCompat
{
    #region Constants

    private const string LogTag = "mashin.MediaBrowserService";

    private const string RootId = "root";

    private const string NodeHome = "node:home";
    private const string NodeDiscover = "node:discover";
    private const string NodePlaylists = "node:playlists";
    private const string NodeFavorites = "node:favorites";

    private const string NodeHomeRecommendations = "node:home:recommendations";
    private const string NodeHomeFavoriteGenres = "node:home:favorite_genres";
    private const string NodeHomePlaylistSuggestions = "node:home:playlist_suggestions";
    private const string NodeHomeTopTracks = "node:home:top_tracks";
    private const string NodeHomeTopArtists = "node:home:top_artists";
    private const string NodeHomeRecentTracks = "node:home:recent_tracks";

    private const string NodeFavoritesTracks = "node:favorites:tracks";
    private const string NodeFavoritesAlbums = "node:favorites:albums";
    private const string NodeFavoritesPlaylists = "node:favorites:playlists";
    private const string NodeFavoritesArtists = "node:favorites:artists";

    private const string PrefixPlaylist = "playlist";
    private const string PrefixTrack = "track";
    private const string PrefixAlbum = "album";
    private const string PrefixArtist = "artist";
    private const string PrefixTrackListAction = "track_action";
    private const string TrackListActionPlay = "play";
    private const string TrackListActionShuffle = "shuffle";

    private const string PrefixArtistAlbums = "artist_albums";
    private const string PrefixArtistSimilar = "artist_similar";
    private const string PrefixArtistTopTracks = "artist_top_tracks";
    private const string PrefixArtistRadio = "artist_radio";

    private const string CustomActionToggleFavorite = "custom:toggle_favorite";
    private const string CustomActionToggleShuffle = "custom:toggle_shuffle";
    private const string CustomActionToggleRepeatMode = "custom:toggle_repeat_mode";
    private const string ExtrasKeyCommandButtonIconCompat = "androidx.media3.session.EXTRAS_KEY_COMMAND_BUTTON_ICON_COMPAT";
    private const int CommandButtonIconHeartFilled = 1042557;
    private const int CommandButtonIconHeartUnfilled = 59517;

    #endregion

    #region Fields

    private readonly object _cacheLock = new();
    private readonly Dictionary<string, mashin.Models.MediaItem> _mediaItemCache = new(StringComparer.Ordinal);
    private readonly Dictionary<string, List<mashin.Models.MediaItem>> _trackListActionCache = new(StringComparer.Ordinal);

    private PlaybackService? _playbackService;
    private MusicAssistantService? _musicAssistantService;
    private UserDataService? _userDataService;
    private SettingsService? _settingsService;
    private MediaSessionCompat? _mediaSession;

    #endregion

    #region Service Lifecycle

    public override void OnCreate()
    {
        base.OnCreate();

        var services = IPlatformApplication.Current?.Services;
        _playbackService = services?.GetService<PlaybackService>();
        _musicAssistantService = services?.GetService<MusicAssistantService>();
        _userDataService = services?.GetService<UserDataService>();
        _settingsService = services?.GetService<SettingsService>();

        EnsureMediaSession();
        EnsurePlaybackInitialized();

        if (_playbackService != null)
        {
            global::Android.Util.Log.Warn(LogTag, "MediaBrowserService OnCreate: PlaybackService resolved, attaching listeners.");
            _playbackService.PropertyChanged += OnPlaybackPropertyChanged;
            SyncMediaSessionState();
        }
        else
        {
            global::Android.Util.Log.Warn(LogTag, "MediaBrowserService OnCreate: PlaybackService is NULL.");
        }
    }

    public override void OnDestroy()
    {
        if (_playbackService != null)
        {
            _playbackService.PropertyChanged -= OnPlaybackPropertyChanged;
        }

        if (_mediaSession != null)
        {
            _mediaSession.Active = false;
            _mediaSession.Release();
            _mediaSession.Dispose();
            _mediaSession = null;
        }

        base.OnDestroy();
    }

    #endregion

    #region Media Browser API

    public override BrowserRoot? OnGetRoot(string? clientPackageName, int clientUid, Bundle? rootHints)
    {
        if (!IsKnownCaller(clientPackageName, clientUid))
        {
            global::Android.Util.Log.Warn(LogTag, $"Rejected browser client package={clientPackageName}, uid={clientUid}");
            return null;
        }

        var extras = BuildContentStyleExtras(
            MediaConstants.DescriptionExtrasValueContentStyleListItem,
            MediaConstants.DescriptionExtrasValueContentStyleListItem);

        return new BrowserRoot(RootId, extras);
    }

    public override void OnLoadChildren(string? parentId, Result? result)
    {
        if (string.IsNullOrWhiteSpace(parentId) || result == null)
        {
            result?.SendResult(new JavaList<MediaBrowserCompat.MediaItem>());
            return;
        }

        if (string.Equals(parentId, RootId, StringComparison.Ordinal))
        {
            result.SendResult(BuildRootItems());
            return;
        }

        result.Detach();
        _ = Task.Run(async () =>
        {
            try
            {
                var items = await LoadChildrenAsync(parentId);
                result.SendResult(items);
            }
            catch (Exception ex)
            {
                global::Android.Util.Log.Warn(LogTag, $"onLoadChildren failed for parent={parentId}: {ex.Message}");
                result.SendResult(new JavaList<MediaBrowserCompat.MediaItem>());
            }
        });
    }

    #endregion

    #region Root And Section Lists

    private JavaList<MediaBrowserCompat.MediaItem> BuildRootItems()
    {
        var items = new List<MediaBrowserCompat.MediaItem>
        {
            CreateBrowsableItem(NodeHome, "Home", "Sektionen", Resource.Drawable.home),
            CreateBrowsableItem(NodeDiscover, "Entdecken", "Demnächst", Resource.Drawable.explore),
            CreateBrowsableItem(
                NodePlaylists,
                "Playlists",
                "Deine Playlists",
                Resource.Drawable.playlist_play,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(NodeFavorites, "Favoriten", "Titel, Alben, Playlists, Artists", Resource.Drawable.favorite)
        };

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private JavaList<MediaBrowserCompat.MediaItem> BuildHomeSectionItems()
    {
        var items = new List<MediaBrowserCompat.MediaItem>
        {
            CreateBrowsableItem(NodeHomeRecommendations, "Empfehlungen", string.Empty, Resource.Drawable.music_note_2),
            CreateBrowsableItem(NodeHomeFavoriteGenres, "Lieblingsgenres", string.Empty, Resource.Drawable.genres),
            CreateBrowsableItem(NodeHomePlaylistSuggestions, "Playlist-Empfehlungen", string.Empty, Resource.Drawable.playlist_play),
            CreateBrowsableItem(NodeHomeTopTracks, "Top-Tracks", string.Empty, Resource.Drawable.music_note),
            CreateBrowsableItem(
                NodeHomeTopArtists,
                "Top-Artists",
                string.Empty,
                Resource.Drawable.artist,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(NodeHomeRecentTracks, "Kürzlich gespielt", string.Empty, Resource.Drawable.music_history)
        };

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private JavaList<MediaBrowserCompat.MediaItem> BuildFavoritesSectionItems()
    {
        var items = new List<MediaBrowserCompat.MediaItem>
        {
            CreateBrowsableItem(
                NodeFavoritesTracks,
                "Tracks",
                "Favorisierte Tracks",
                Resource.Drawable.music_note,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleListItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(
                NodeFavoritesAlbums,
                "Alben",
                "Favorisierte Alben",
                Resource.Drawable.album,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(
                NodeFavoritesPlaylists,
                "Playlists",
                "Favorisierte Playlists",
                Resource.Drawable.playlist_play,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(
                NodeFavoritesArtists,
                "Artists",
                "Favorisierte Artists",
                Resource.Drawable.artist,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem))
        };

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private JavaList<MediaBrowserCompat.MediaItem> BuildArtistSectionItems(string provider, string artistId, string artistName)
    {
        var artistRadioNode = BuildId(PrefixArtistRadio, provider, artistId);
        var albumsNode = BuildId(PrefixArtistAlbums, provider, artistId);
        var similarNode = BuildId(PrefixArtistSimilar, provider, artistId);
        var topTracksNode = BuildId(PrefixArtistTopTracks, provider, artistId);

        var items = new List<MediaBrowserCompat.MediaItem>
        {
            CreatePlayableItem(
                artistRadioNode,
                "Artist-Radio",
                string.Empty,
                Resource.Drawable.radio),
            CreateBrowsableItem(
                albumsNode,
                "Alben",
                string.Empty,
                Resource.Drawable.album,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(
                similarNode,
                "Ähnliche Artists",
                string.Empty,
                Resource.Drawable.artist,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleGridItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem)),
            CreateBrowsableItem(topTracksNode, "Top-Tracks", string.Empty, Resource.Drawable.music_note)
        };

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    #endregion

    #region Children Loading Routing

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadChildrenAsync(string parentId)
    {
        if (string.Equals(parentId, NodeHome, StringComparison.Ordinal))
        {
            return BuildHomeSectionItems();
        }

        if (string.Equals(parentId, NodeDiscover, StringComparison.Ordinal))
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        if (string.Equals(parentId, NodePlaylists, StringComparison.Ordinal))
        {
            return await LoadPlaylistItemsAsync(favoriteOnly: false);
        }

        if (string.Equals(parentId, NodeFavoritesPlaylists, StringComparison.Ordinal))
        {
            return await LoadPlaylistItemsAsync(favoriteOnly: true);
        }

        if (string.Equals(parentId, NodeHomeFavoriteGenres, StringComparison.Ordinal))
        {
            return await LoadGenrePlaylistsAsync();
        }

        if (string.Equals(parentId, NodeHomePlaylistSuggestions, StringComparison.Ordinal))
        {
            return await LoadArtistPlaylistsAsync();
        }

        if (string.Equals(parentId, NodeFavorites, StringComparison.Ordinal))
        {
            return BuildFavoritesSectionItems();
        }

        if (string.Equals(parentId, NodeFavoritesTracks, StringComparison.Ordinal))
        {
            return await LoadFavoriteTracksAsync();
        }

        if (string.Equals(parentId, NodeHomeRecommendations, StringComparison.Ordinal))
        {
            return await LoadRecommendationTracksAsync();
        }

        if (string.Equals(parentId, NodeHomeTopTracks, StringComparison.Ordinal))
        {
            return await LoadTopTracksAsync();
        }

        if (string.Equals(parentId, NodeHomeRecentTracks, StringComparison.Ordinal))
        {
            return await LoadRecentListensAsync();
        }

        if (string.Equals(parentId, NodeFavoritesAlbums, StringComparison.Ordinal))
        {
            return await LoadAlbumItemsAsync(favoriteOnly: true);
        }

        if (string.Equals(parentId, NodeFavoritesArtists, StringComparison.Ordinal))
        {
            return await LoadArtistItemsAsync(favoriteOnly: string.Equals(parentId, NodeFavoritesArtists, StringComparison.Ordinal));
        }

        if (string.Equals(parentId, NodeHomeTopArtists, StringComparison.Ordinal))
        {
            return await LoadTopArtistsAsync();
        }

        if (TryParseId(parentId, out var type, out var provider, out var itemId))
        {
            if (string.Equals(type, PrefixPlaylist, StringComparison.Ordinal))
            {
                return await LoadPlaylistTracksAsync(provider, itemId);
            }

            if (string.Equals(type, PrefixAlbum, StringComparison.Ordinal))
            {
                return await LoadAlbumTracksAsync(provider, itemId);
            }

            if (string.Equals(type, PrefixArtist, StringComparison.Ordinal))
            {
                return await LoadArtistSectionsAsync(provider, itemId);
            }

            if (string.Equals(type, PrefixArtistRadio, StringComparison.Ordinal))
            {
                return new JavaList<MediaBrowserCompat.MediaItem>();
            }

            if (string.Equals(type, PrefixArtistAlbums, StringComparison.Ordinal))
            {
                return await LoadArtistAlbumsAsync(provider, itemId);
            }

            if (string.Equals(type, PrefixArtistSimilar, StringComparison.Ordinal))
            {
                return await LoadSimilarArtistsAsync(provider, itemId);
            }

            if (string.Equals(type, PrefixArtistTopTracks, StringComparison.Ordinal))
            {
                return await LoadArtistTopTracksAsync(provider, itemId);
            }
        }

        return new JavaList<MediaBrowserCompat.MediaItem>();
    }

    #endregion

    #region Data Loading

    #region Home Data Loading

    private Task<JavaList<MediaBrowserCompat.MediaItem>> LoadRecommendationTracksAsync()
    {
        return LoadTracksFromRecommendationFolderAsync("recommendations");
    }

    private Task<JavaList<MediaBrowserCompat.MediaItem>> LoadTopTracksAsync()
    {
        return LoadTracksFromRecommendationFolderAsync("top_tracks");
    }

    private Task<JavaList<MediaBrowserCompat.MediaItem>> LoadGenrePlaylistsAsync()
    {
        return LoadPlaylistsFromRecommendationFolderAsync("genre_playlists", takeLimit: 9, applyNameNormalization: true);
    }

    private Task<JavaList<MediaBrowserCompat.MediaItem>> LoadArtistPlaylistsAsync()
    {
        return LoadPlaylistsFromRecommendationFolderAsync("artist_playlists", takeLimit: null, applyNameNormalization: true);
    }

    private Task<JavaList<MediaBrowserCompat.MediaItem>> LoadTopArtistsAsync()
    {
        return LoadArtistsFromRecommendationFolderAsync("top_artists");
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadTracksFromRecommendationFolderAsync(string folderId)
    {
        var folders = await LoadListenBrainzRecommendationFoldersAsync();
        var tracks = FindRecommendationFolderById(folders, folderId)?.Items?
            .OfType<Track>()
            .Where(track => !string.IsNullOrWhiteSpace(track.ItemId) && !string.IsNullOrWhiteSpace(track.Provider))
            .ToList()
            ?? new List<Track>();

        if (tracks.Count == 0)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        return BuildTrackItems($"home:{folderId}", tracks);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadRecentListensAsync()
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var currentUser = await musicAssistant.GetCurrentUserAsync();
        var recentItems = await musicAssistant.GetRecentlyPlayedItemsAsync(
            limit: 50,
            mediaTypes: new[] { MediaType.Track },
            userId: currentUser?.UserId);

        var trackRefs = recentItems
            .OfType<Track>()
            .Where(track => !string.IsNullOrWhiteSpace(track.ItemId) && !string.IsNullOrWhiteSpace(track.Provider))
            .ToList();

        var fullTrackTasks = trackRefs.Select(async trackRef =>
        {
            try
            {
                return await musicAssistant.GetTrackAsync(trackRef.ItemId, trackRef.Provider) ?? trackRef;
            }
            catch
            {
                return trackRef;
            }
        });

        var tracks = (await Task.WhenAll(fullTrackTasks))
            .Where(track => track != null)
            .Select(track => track!)
            .ToList();

        if (tracks.Count == 0)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        return BuildTrackItems(NodeHomeRecentTracks, tracks);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadPlaylistsFromRecommendationFolderAsync(
        string folderId,
        int? takeLimit,
        bool applyNameNormalization)
    {
        var folders = await LoadListenBrainzRecommendationFoldersAsync();
        var playlists = FindRecommendationFolderById(folders, folderId)?.Items?
            .OfType<Playlist>()
            .Where(playlist => !string.IsNullOrWhiteSpace(playlist.ItemId) && !string.IsNullOrWhiteSpace(playlist.Provider))
            .ToList()
            ?? new List<Playlist>();

        if (takeLimit.HasValue)
        {
            playlists = playlists.Take(takeLimit.Value).ToList();
        }

        var items = new List<MediaBrowserCompat.MediaItem>();
        foreach (var playlist in playlists)
        {
            if (applyNameNormalization)
            {
                playlist.DisplayName = NormalizePlaylistDisplayName(playlist.Name);
            }

            var mediaId = BuildId(PrefixPlaylist, playlist.Provider, playlist.ItemId);
            CacheMediaItem(mediaId, playlist);

            var title = SelectDisplayName(playlist, "Playlist");

            items.Add(CreateBrowsableItem(
                mediaId,
                title,
                string.Empty,
                Resource.Drawable.playlist_play,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleListItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem),
                ResolveArtworkUri(playlist, Resource.Drawable.playlist_play)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadArtistsFromRecommendationFolderAsync(string folderId)
    {
        var folders = await LoadListenBrainzRecommendationFoldersAsync();
        var artists = FindRecommendationFolderById(folders, folderId)?.Items?
            .OfType<Artist>()
            .Where(artist => !string.IsNullOrWhiteSpace(artist.ItemId) && !string.IsNullOrWhiteSpace(artist.Provider))
            .ToList()
            ?? new List<Artist>();

        var items = new List<MediaBrowserCompat.MediaItem>();
        foreach (var artist in artists)
        {
            var mediaId = BuildId(PrefixArtist, artist.Provider, artist.ItemId);
            CacheMediaItem(mediaId, artist);

            items.Add(CreateBrowsableItem(
                mediaId,
                SelectDisplayName(artist, "Artist"),
                string.Empty,
                Resource.Drawable.favorite,
                iconUri: ResolveArtworkUri(artist, Resource.Drawable.favorite)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<List<RecommendationFolder>> LoadListenBrainzRecommendationFoldersAsync()
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new List<RecommendationFolder>();
        }

        var recommendations = await musicAssistant.GetRecommendationsAsync();
        return recommendations
            .Where(IsListenBrainzFolder)
            .ToList();
    }

    #endregion

    #region Catalog Data Loading

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadPlaylistItemsAsync(bool favoriteOnly)
    {
        if (favoriteOnly)
        {
            var userDataService = _userDataService;
            if (userDataService == null)
            {
                return new JavaList<MediaBrowserCompat.MediaItem>();
            }

            var favoritePlaylists = userDataService.FavoritePlaylists.ToList();

            var musicAssistant = _musicAssistantService;
            if (musicAssistant != null && favoritePlaylists.Count > 0)
            {
                await musicAssistant.EnrichWithProviderInfoAsync(favoritePlaylists);
            }

            var favoriteItems = new List<MediaBrowserCompat.MediaItem>();
            foreach (var playlist in favoritePlaylists.Where(playlist => !string.IsNullOrWhiteSpace(playlist.ItemId) && !string.IsNullOrWhiteSpace(playlist.Provider)))
            {
                var mediaId = BuildId(PrefixPlaylist, playlist.Provider, playlist.ItemId);
                CacheMediaItem(mediaId, playlist);

                favoriteItems.Add(CreateBrowsableItem(
                    mediaId,
                    SelectDisplayName(playlist, "Playlist"),
                    $"{Math.Max(0, playlist.TracksCount)} Titel",
                    Resource.Drawable.playlist_play,
                    BuildContentStyleExtras(
                        MediaConstants.DescriptionExtrasValueContentStyleListItem,
                        MediaConstants.DescriptionExtrasValueContentStyleListItem),
                    ResolveArtworkUri(playlist, Resource.Drawable.playlist_play)));
            }

            return new JavaList<MediaBrowserCompat.MediaItem>(favoriteItems);
        }

        var userDataService = _userDataService;
        if (userDataService == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var playlists = userDataService.Playlists;

        var items = new List<MediaBrowserCompat.MediaItem>();
        foreach (var playlist in playlists)
        {
            var mediaId = BuildId(PrefixPlaylist, playlist.Provider, playlist.ItemId);
            CacheMediaItem(mediaId, playlist);

            items.Add(CreateBrowsableItem(
                mediaId,
                SelectDisplayName(playlist, "Playlist"),
                $"{Math.Max(0, playlist.TracksCount)} Titel",
                Resource.Drawable.playlist_play,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleListItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem),
                ResolveArtworkUri(playlist, Resource.Drawable.playlist_play)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadFavoriteTracksAsync()
    {
        var userDataService = _userDataService;
        if (userDataService == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var tracks = userDataService.FavoriteTracks.ToList();

        var musicAssistant = _musicAssistantService;
        if (musicAssistant != null && tracks.Count > 0)
        {
            await musicAssistant.EnrichWithProviderInfoAsync(tracks);
        }

        for (var i = 0; i < tracks.Count; i++)
        {
            tracks[i].Index = i;
            tracks[i].Favorite = true;
        }

        return BuildTrackItems(NodeFavoritesTracks, tracks);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadPlaylistTracksAsync(string provider, string itemId)
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var tracks = await musicAssistant.GetPlaylistTracksAsync(itemId, provider);
        return BuildTrackItems(BuildId(PrefixPlaylist, provider, itemId), tracks);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadAlbumItemsAsync(bool favoriteOnly)
    {
        if (favoriteOnly)
        {
            var userDataService = _userDataService;
            if (userDataService == null)
            {
                return new JavaList<MediaBrowserCompat.MediaItem>();
            }

            var favoriteAlbums = userDataService.FavoriteAlbums.ToList();

            var favoriteMusicAssistant = _musicAssistantService;
            if (favoriteMusicAssistant != null && favoriteAlbums.Count > 0)
            {
                await favoriteMusicAssistant.EnrichWithProviderInfoAsync(favoriteAlbums);
            }

            var favoriteItems = new List<MediaBrowserCompat.MediaItem>();
            foreach (var album in favoriteAlbums.Where(album => !string.IsNullOrWhiteSpace(album.ItemId) && !string.IsNullOrWhiteSpace(album.Provider)))
            {
                var mediaId = BuildId(PrefixAlbum, album.Provider, album.ItemId);
                CacheMediaItem(mediaId, album);

                favoriteItems.Add(CreateBrowsableItem(
                    mediaId,
                    SelectDisplayName(album, "Album"),
                    album.ArtistName,
                    Resource.Drawable.album,
                    BuildContentStyleExtras(
                        MediaConstants.DescriptionExtrasValueContentStyleListItem,
                        MediaConstants.DescriptionExtrasValueContentStyleListItem),
                    ResolveArtworkUri(album, Resource.Drawable.album)));
            }

            return new JavaList<MediaBrowserCompat.MediaItem>(favoriteItems);
        }

        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var albums = await musicAssistant.GetLibraryAlbumsAsync(
            favorite: favoriteOnly ? true : null,
            limit: 200,
            orderBy: "sort_name",
            libraryItemsOnly: true);

        var items = new List<MediaBrowserCompat.MediaItem>();
        foreach (var album in albums)
        {
            var mediaId = BuildId(PrefixAlbum, album.Provider, album.ItemId);
            CacheMediaItem(mediaId, album);

            items.Add(CreateBrowsableItem(
                mediaId,
                SelectDisplayName(album, "Album"),
                album.ArtistName,
                Resource.Drawable.album,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleListItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem),
                ResolveArtworkUri(album, Resource.Drawable.album)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadArtistItemsAsync(bool favoriteOnly)
    {
        if (favoriteOnly)
        {
            var userDataService = _userDataService;
            if (userDataService == null)
            {
                return new JavaList<MediaBrowserCompat.MediaItem>();
            }

            var favoriteArtists = userDataService.FavoriteArtists.ToList();

            var favoriteMusicAssistant = _musicAssistantService;
            if (favoriteMusicAssistant != null && favoriteArtists.Count > 0)
            {
                await favoriteMusicAssistant.EnrichWithProviderInfoAsync(favoriteArtists);
            }

            var favoriteItems = new List<MediaBrowserCompat.MediaItem>();
            foreach (var artist in favoriteArtists.Where(artist => !string.IsNullOrWhiteSpace(artist.ItemId) && !string.IsNullOrWhiteSpace(artist.Provider)))
            {
                var mediaId = BuildId(PrefixArtist, artist.Provider, artist.ItemId);
                CacheMediaItem(mediaId, artist);

                favoriteItems.Add(CreateBrowsableItem(
                    mediaId,
                    SelectDisplayName(artist, "Artist"),
                    string.Empty,
                    Resource.Drawable.artist,
                    iconUri: ResolveArtworkUri(artist, Resource.Drawable.artist)));
            }

            return new JavaList<MediaBrowserCompat.MediaItem>(favoriteItems);
        }

        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var artists = await musicAssistant.GetLibraryArtistsAsync(
            favorite: favoriteOnly ? true : null,
            limit: 200,
            orderBy: "sort_name",
            libraryItemsOnly: true);

        var items = new List<MediaBrowserCompat.MediaItem>();
        foreach (var artist in artists)
        {
            var mediaId = BuildId(PrefixArtist, artist.Provider, artist.ItemId);
            CacheMediaItem(mediaId, artist);

            items.Add(CreateBrowsableItem(
                mediaId,
                SelectDisplayName(artist, "Artist"),
                string.Empty,
                Resource.Drawable.artist,
                iconUri: ResolveArtworkUri(artist, Resource.Drawable.artist)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadAlbumTracksAsync(string provider, string itemId)
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var tracks = await musicAssistant.GetAlbumTracksAsync(itemId, provider, inLibraryOnly: false);
        return BuildTrackItems(BuildId(PrefixAlbum, provider, itemId), tracks);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadArtistSectionsAsync(string provider, string itemId)
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var artist = await musicAssistant.GetArtistAsync(itemId, provider);
        var artistName = SelectDisplayName(artist ?? new Artist { Name = "Artist" }, "Artist");

        return BuildArtistSectionItems(provider, itemId, artistName);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadArtistAlbumsAsync(string provider, string itemId)
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var albums = await musicAssistant.GetArtistAlbumsAsync(itemId, provider);
        if (albums.Count == 0)
        {
            albums = await musicAssistant.GetArtistTopAlbumsAsync(itemId, provider);
        }

        albums = albums
            .OrderByDescending(album => album.Year ?? 0)
            .ToList();

        var items = new List<MediaBrowserCompat.MediaItem>();

        foreach (var album in albums)
        {
            var mediaId = BuildId(PrefixAlbum, album.Provider, album.ItemId);
            CacheMediaItem(mediaId, album);

            items.Add(CreateBrowsableItem(
                mediaId,
                SelectDisplayName(album, "Album"),
                album.ArtistName,
                Resource.Drawable.playlist_play,
                BuildContentStyleExtras(
                    MediaConstants.DescriptionExtrasValueContentStyleListItem,
                    MediaConstants.DescriptionExtrasValueContentStyleListItem),
                ResolveArtworkUri(album, Resource.Drawable.playlist_play)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadSimilarArtistsAsync(string provider, string itemId)
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var artists = await musicAssistant.GetSimilarArtistsAsync(itemId, provider, limit: 100);

        if (artists.Count == 0)
        {
            var topTracks = await musicAssistant.GetArtistTopTracksAsync(itemId, provider);
            var topTrack = topTracks.FirstOrDefault();

            var similarTracks = topTrack != null
                ? await musicAssistant.GetSimilarTracksAsync(
                    topTrack.ItemId,
                    topTrack.Provider,
                    limit: 50,
                    allowLookup: true)
                : new List<Track>();

            if (topTrack != null && similarTracks.Count == 0)
            {
                var versions = await musicAssistant.GetTrackVersionsAsync(topTrack.ItemId, topTrack.Provider);
                var fallbackVersion = versions
                    .FirstOrDefault(version => !string.Equals(version.Provider, topTrack.Provider, StringComparison.OrdinalIgnoreCase))
                    ?? versions.FirstOrDefault();

                if (fallbackVersion != null)
                {
                    similarTracks = await musicAssistant.GetSimilarTracksAsync(
                        fallbackVersion.ItemId,
                        fallbackVersion.Provider,
                        limit: 50,
                        allowLookup: true);
                }
            }

            artists = similarTracks
                .SelectMany(track => track.Artists ?? Enumerable.Empty<Artist>())
                .GroupBy(artist => string.Concat(artist.Provider, "|", artist.ItemId))
                .Select(group => group.First())
                .Where(artist => !string.Equals(artist.ItemId, itemId, StringComparison.Ordinal))
                .Take(100)
                .ToList();

            if (artists.Count > 0)
            {
                var enriched = await Task.WhenAll(artists.Select(async artist =>
                {
                    try
                    {
                        return await musicAssistant.GetArtistAsync(artist.ItemId, artist.Provider) ?? artist;
                    }
                    catch
                    {
                        return artist;
                    }
                }));

                artists = enriched.ToList();
            }
        }

        var items = new List<MediaBrowserCompat.MediaItem>();

        foreach (var artist in artists)
        {
            var mediaId = BuildId(PrefixArtist, artist.Provider, artist.ItemId);
            CacheMediaItem(mediaId, artist);

            items.Add(CreateBrowsableItem(
                mediaId,
                SelectDisplayName(artist, "Artist"),
                string.Empty,
                Resource.Drawable.favorite,
                iconUri: ResolveArtworkUri(artist, Resource.Drawable.favorite)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    private async Task<JavaList<MediaBrowserCompat.MediaItem>> LoadArtistTopTracksAsync(string provider, string itemId)
    {
        var musicAssistant = _musicAssistantService;
        if (musicAssistant == null)
        {
            return new JavaList<MediaBrowserCompat.MediaItem>();
        }

        var tracks = await musicAssistant.GetArtistTopTracksAsync(itemId, provider);
        return BuildTrackItems(BuildId(PrefixArtistTopTracks, provider, itemId), tracks);
    }

    private JavaList<MediaBrowserCompat.MediaItem> BuildTrackItems(string sourceKey, IEnumerable<Track> tracks)
    {
        var trackList = tracks
            .Where(track => !string.IsNullOrWhiteSpace(track.ItemId) && !string.IsNullOrWhiteSpace(track.Provider))
            .ToList();

        var items = new List<MediaBrowserCompat.MediaItem>();
        AddTrackListActionItems(items, sourceKey, trackList);

        foreach (var track in trackList)
        {
            var mediaId = BuildId(PrefixTrack, track.Provider, track.ItemId);
            CacheMediaItem(mediaId, track);

            items.Add(CreatePlayableItem(
                mediaId,
                SelectDisplayName(track, "Titel"),
                track.ArtistName,
                Resource.Drawable.play_arrow,
                iconUri: ResolveArtworkUri(track, Resource.Drawable.play_arrow)));
        }

        return new JavaList<MediaBrowserCompat.MediaItem>(items);
    }

    #endregion

    #endregion

    #region Session And Playback Sync

    private void EnsureMediaSession()
    {
        if (_mediaSession != null)
        {
            return;
        }

        _mediaSession = new MediaSessionCompat(this, "mashin-media-browser-session");
        _mediaSession.SetCallback(new MediaSessionCallback(this));
        _mediaSession.Active = true;

        SessionToken = _mediaSession.SessionToken;
    }

    private void EnsurePlaybackInitialized()
    {
        var playback = _playbackService;
        if (playback == null)
        {
            return;
        }

        _ = Task.Run(async () =>
        {
            try
            {
                await playback.InitializeAsync();
                await playback.SetOutputModeAsync(PlaybackOutputMode.Sendspin);
            }
            catch (Exception ex)
            {
                global::Android.Util.Log.Warn(LogTag, $"Playback initialization failed: {ex.Message}");
            }
        });
    }

    private void OnPlaybackPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(PlaybackService.PlaybackState)
            || e.PropertyName == nameof(PlaybackService.CurrentQueueItem)
            || e.PropertyName == nameof(PlaybackService.PositionSeconds)
            || e.PropertyName == nameof(PlaybackService.DurationSeconds)
            || e.PropertyName == nameof(PlaybackService.ShuffleEnabled)
            || e.PropertyName == nameof(PlaybackService.RepeatMode)
            || e.PropertyName == nameof(PlaybackService.CurrentQueueItems)
            || e.PropertyName == nameof(PlaybackService.CurrentQueueIndex))
        {
            SyncMediaSessionState();

            if (e.PropertyName == nameof(PlaybackService.CurrentQueueItem))
            {
                _ = RefreshCurrentTrackFavoriteStateAsync();
            }
        }
    }

    private void SyncMediaSessionState()
    {
        var session = _mediaSession;
        var playback = _playbackService;
        if (session == null || playback == null)
        {
            return;
        }

        var track = playback.CurrentQueueItem?.MediaItem;
        var state = playback.PlaybackState.State;
        var durationSeconds = Math.Max(0, playback.DurationSeconds);
        var positionSeconds = Math.Clamp(playback.PositionSeconds, 0, durationSeconds > 0 ? durationSeconds : double.MaxValue);

        var playbackStateBuilder = new PlaybackStateCompat.Builder()
            .SetActions(
                PlaybackStateCompat.ActionPlay
                | PlaybackStateCompat.ActionPause
                | PlaybackStateCompat.ActionPlayFromMediaId
                | PlaybackStateCompat.ActionPlayFromSearch
                | PlaybackStateCompat.ActionSkipToNext
                | PlaybackStateCompat.ActionSkipToPrevious
                | PlaybackStateCompat.ActionSkipToQueueItem
                | PlaybackStateCompat.ActionStop)
            .SetState(MapPlaybackState(state), (long)(positionSeconds * 1000d), state == PlayerStateType.Playing ? 1f : 0f, SystemClock.ElapsedRealtime());

        if (playback.CurrentQueueIndex is int currentQueueIndex && currentQueueIndex >= 0)
        {
            playbackStateBuilder.SetActiveQueueItemId(currentQueueIndex);
        }

        var favoriteActionExtras = new Bundle();
        favoriteActionExtras.PutInt(
            ExtrasKeyCommandButtonIconCompat,
            track?.Favorite == true ? CommandButtonIconHeartFilled : CommandButtonIconHeartUnfilled);

        var isShuffleEnabled = playback.ShuffleEnabled == true;
        var shuffleActionExtras = new Bundle();
        shuffleActionExtras.PutInt(
            ExtrasKeyCommandButtonIconCompat,
            isShuffleEnabled ? Resource.Drawable.shuffle_on : Resource.Drawable.shuffle);

        var repeatMode = GetNormalizedRepeatMode(playback.RepeatMode);
        var repeatActionExtras = new Bundle();
        repeatActionExtras.PutInt(
            ExtrasKeyCommandButtonIconCompat,
            GetRepeatModeIconResource(repeatMode));

        var repeatActionLabel = repeatMode switch
        {
            mashin.Models.RepeatMode.Off => "Repeat aktivieren",
            mashin.Models.RepeatMode.All => "Repeat One aktivieren",
            _ => "Repeat deaktivieren"
        };

        playbackStateBuilder
            .AddCustomAction(new PlaybackStateCompat.CustomAction.Builder(
                CustomActionToggleFavorite,
                new Java.Lang.String(track?.Favorite == true ? "Favorit entfernen" : "Zu Favoriten"),
                track?.Favorite == true
                    ? Resource.Drawable.favorite_filled
                    : Resource.Drawable.favorite)
                .SetExtras(favoriteActionExtras)
                .Build())
            .AddCustomAction(new PlaybackStateCompat.CustomAction.Builder(
                CustomActionToggleShuffle,
                new Java.Lang.String(isShuffleEnabled ? "Shuffle deaktivieren" : "Shuffle aktivieren"),
                isShuffleEnabled
                    ? Resource.Drawable.shuffle_on
                    : Resource.Drawable.shuffle)
                .SetExtras(shuffleActionExtras)
                .Build())
            .AddCustomAction(new PlaybackStateCompat.CustomAction.Builder(
                CustomActionToggleRepeatMode,
                new Java.Lang.String(repeatActionLabel),
                GetRepeatModeIconResource(repeatMode))
                .SetExtras(repeatActionExtras)
                .Build());

        session.SetPlaybackState(playbackStateBuilder.Build());

        var metadataBuilder = new MediaMetadataCompat.Builder()
            .PutString(MediaMetadataCompat.MetadataKeyTitle, track?.DisplayName ?? track?.Name ?? string.Empty)
            .PutString(MediaMetadataCompat.MetadataKeyArtist, track?.ArtistName ?? string.Empty)
            .PutString(MediaMetadataCompat.MetadataKeyAlbum, track?.AlbumName ?? string.Empty)
            .PutLong(MediaMetadataCompat.MetadataKeyDuration, (long)(durationSeconds * 1000d));

        var artworkMetadataUri = ResolveArtworkUriString(track);
        if (!string.IsNullOrWhiteSpace(artworkMetadataUri))
        {
            metadataBuilder
                .PutString(MediaMetadataCompat.MetadataKeyDisplayIconUri, artworkMetadataUri)
                .PutString(MediaMetadataCompat.MetadataKeyArtUri, artworkMetadataUri)
                .PutString(MediaMetadataCompat.MetadataKeyAlbumArtUri, artworkMetadataUri);
        }

        session.SetMetadata(metadataBuilder.Build());

        var queue = BuildSessionQueue(playback.CurrentQueueItems);
        session.SetQueue(queue);
        session.SetQueueTitle("Wiedergabeliste");
    }

    private async Task RefreshCurrentTrackFavoriteStateAsync()
    {
        var playback = _playbackService;
        var userDataService = _userDataService;
        var currentMediaItem = playback?.CurrentQueueItem?.MediaItem;

        if (playback == null
            || userDataService == null
            || currentMediaItem == null
            || string.IsNullOrWhiteSpace(currentMediaItem.Uri))
        {
            return;
        }

        try
        {
            var isFavorite = await userDataService.IsFavoriteAsync(currentMediaItem);
            if (currentMediaItem.Favorite != isFavorite)
            {
                currentMediaItem.Favorite = isFavorite;
                SyncMediaSessionState();
            }
        }
        catch
        {
        }
    }

    private JavaList<MediaSessionCompat.QueueItem> BuildSessionQueue(IEnumerable<QueueItem> queueItems)
    {
        var items = new List<MediaSessionCompat.QueueItem>();
        var index = 0L;

        foreach (var queueItem in queueItems)
        {
            var mediaItem = queueItem.MediaItem;
            if (mediaItem == null)
            {
                continue;
            }

            var description = new MediaDescriptionCompat.Builder()
                .SetMediaId(string.IsNullOrWhiteSpace(mediaItem.ItemId) ? queueItem.QueueItemId : mediaItem.ItemId)
                .SetTitle(SelectDisplayName(mediaItem, "Titel"))
                .SetSubtitle(mediaItem.ArtistName)
                .SetIconUri(ResolveArtworkUri(mediaItem, Resource.Drawable.equalizer))
                .Build();

            items.Add(new MediaSessionCompat.QueueItem(description, index));
            index++;
        }

        return new JavaList<MediaSessionCompat.QueueItem>(items);
    }

    private static int MapPlaybackState(PlayerStateType state)
    {
        return state switch
        {
            PlayerStateType.Playing => PlaybackStateCompat.StatePlaying,
            PlayerStateType.Paused => PlaybackStateCompat.StatePaused,
            PlayerStateType.Buffering => PlaybackStateCompat.StateBuffering,
            PlayerStateType.Seeking => PlaybackStateCompat.StateConnecting,
            PlayerStateType.Error => PlaybackStateCompat.StateError,
            PlayerStateType.Idle => PlaybackStateCompat.StateStopped,
            _ => PlaybackStateCompat.StateNone
        };
    }

    private static mashin.Models.RepeatMode GetNormalizedRepeatMode(string? repeatMode)
    {
        if (Enum.TryParse<mashin.Models.RepeatMode>(repeatMode, true, out var parsedMode))
        {
            return parsedMode;
        }

        return mashin.Models.RepeatMode.Off;
    }

    private static int GetRepeatModeIconResource(mashin.Models.RepeatMode repeatMode)
    {
        return repeatMode switch
        {
            mashin.Models.RepeatMode.All => Resource.Drawable.repeat_on,
            mashin.Models.RepeatMode.One => Resource.Drawable.repeat_one_on,
            _ => Resource.Drawable.repeat
        };
    }

    #endregion

    #region Helpers

    private bool IsKnownCaller(string? clientPackageName, int clientUid)
    {
        if (clientUid == Process.SystemUid)
        {
            return true;
        }

        if (string.IsNullOrWhiteSpace(clientPackageName))
        {
            return false;
        }

        if (string.Equals(clientPackageName, PackageName, StringComparison.Ordinal))
        {
            return true;
        }

        var trustedPackages = new HashSet<string>(StringComparer.Ordinal)
        {
            "com.google.android.projection.gearhead",
            "com.google.android.googlequicksearchbox",
            "com.android.car.media",
            "com.google.android.gms",
            "com.google.android.apps.automotive.inputmethod"
        };

        if (trustedPackages.Contains(clientPackageName))
        {
            return true;
        }

        return false;
    }

    private MediaBrowserCompat.MediaItem CreateBrowsableItem(
        string mediaId,
        string title,
        string subtitle,
        int iconResource,
        Bundle? extras = null,
        global::Android.Net.Uri? iconUri = null)
    {
        var description = new MediaDescriptionCompat.Builder()
            .SetMediaId(mediaId)
            .SetTitle(title)
            .SetSubtitle(subtitle)
            .SetIconUri(iconUri ?? CreateAndroidResourceUri(iconResource))
            .SetExtras(extras)
            .Build();

        return new MediaBrowserCompat.MediaItem(description, MediaBrowserCompat.MediaItem.FlagBrowsable);
    }

    private MediaBrowserCompat.MediaItem CreatePlayableItem(
        string mediaId,
        string title,
        string subtitle,
        int iconResource,
        Bundle? extras = null,
        global::Android.Net.Uri? iconUri = null)
    {
        var description = new MediaDescriptionCompat.Builder()
            .SetMediaId(mediaId)
            .SetTitle(title)
            .SetSubtitle(subtitle)
            .SetIconUri(iconUri ?? CreateAndroidResourceUri(iconResource))
            .SetExtras(extras)
            .Build();

        return new MediaBrowserCompat.MediaItem(description, MediaBrowserCompat.MediaItem.FlagPlayable);
    }

    private static Bundle BuildContentStyleExtras(int browsableStyle, int playableStyle)
    {
        var extras = new Bundle();
        extras.PutInt(MediaConstants.DescriptionExtrasKeyContentStyleBrowsable, browsableStyle);
        extras.PutInt(MediaConstants.DescriptionExtrasKeyContentStylePlayable, playableStyle);
        return extras;
    }

    private global::Android.Net.Uri CreateAndroidResourceUri(int resourceId)
    {
        var resources = ApplicationContext.Resources;

        return new global::Android.Net.Uri.Builder()
            .Scheme(ContentResolver.SchemeAndroidResource)
            .Authority(resources.GetResourcePackageName(resourceId))
            .AppendPath(resources.GetResourceTypeName(resourceId))
            .AppendPath(resources.GetResourceEntryName(resourceId))
            .Build();
    }

    private global::Android.Net.Uri ResolveArtworkUri(mashin.Models.MediaItem? item, int fallbackResourceId)
    {
        var contentUri = BuildArtworkContentUri(item?.PrimaryImage?.Path);
        if (contentUri != null)
        {
            return contentUri;
        }

        return CreateAndroidResourceUri(fallbackResourceId);
    }

    private string? ResolveArtworkUriString(mashin.Models.MediaItem? item)
    {
        return BuildArtworkContentUri(item?.PrimaryImage?.Path)?.ToString();
    }

    private static global::Android.Net.Uri? BuildArtworkContentUri(string? imagePath)
    {
        if (string.IsNullOrWhiteSpace(imagePath))
        {
            return null;
        }

        if (!Uri.TryCreate(imagePath, UriKind.Absolute, out var parsedUri))
        {
            return null;
        }

        if (string.Equals(parsedUri.Scheme, ContentResolver.SchemeContent, StringComparison.OrdinalIgnoreCase)
            || string.Equals(parsedUri.Scheme, ContentResolver.SchemeAndroidResource, StringComparison.OrdinalIgnoreCase))
        {
            return global::Android.Net.Uri.Parse(imagePath);
        }

        if (string.Equals(parsedUri.Scheme, Uri.UriSchemeHttp, StringComparison.OrdinalIgnoreCase)
            || string.Equals(parsedUri.Scheme, Uri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase))
        {
            return MediaArtworkContentProvider.BuildContentUri(imagePath);
        }

        if (string.Equals(parsedUri.Scheme, Uri.UriSchemeData, StringComparison.OrdinalIgnoreCase))
        {
            return MediaArtworkContentProvider.BuildContentUri(imagePath);
        }

        return null;
    }

    private static string SelectDisplayName(mashin.Models.MediaItem item, string fallback)
    {
        if (!string.IsNullOrWhiteSpace(item.DisplayName))
        {
            return item.DisplayName;
        }

        if (!string.IsNullOrWhiteSpace(item.Name))
        {
            return item.Name;
        }

        return fallback;
    }

    private static string BuildId(string type, string provider, string itemId)
    {
        var safeProvider = provider.Replace("|", "%7C", StringComparison.Ordinal);
        var safeItemId = itemId.Replace("|", "%7C", StringComparison.Ordinal);
        return string.Concat(type, "|", safeProvider, "|", safeItemId);
    }

    private static bool TryParseId(string mediaId, out string type, out string provider, out string itemId)
    {
        type = string.Empty;
        provider = string.Empty;
        itemId = string.Empty;

        var parts = mediaId.Split('|');
        if (parts.Length != 3)
        {
            return false;
        }

        type = parts[0];
        provider = parts[1].Replace("%7C", "|", StringComparison.Ordinal);
        itemId = parts[2].Replace("%7C", "|", StringComparison.Ordinal);

        return !string.IsNullOrWhiteSpace(type);
    }

    private void CacheMediaItem(string mediaId, mashin.Models.MediaItem item)
    {
        lock (_cacheLock)
        {
            _mediaItemCache[mediaId] = item;
        }
    }

    private bool TryGetCachedMediaItem(string mediaId, out mashin.Models.MediaItem? item)
    {
        lock (_cacheLock)
        {
            return _mediaItemCache.TryGetValue(mediaId, out item);
        }
    }

    private void CacheTrackListAction(string mediaId, List<mashin.Models.MediaItem> items)
    {
        lock (_cacheLock)
        {
            _trackListActionCache[mediaId] = items;
        }
    }

    private bool TryGetCachedTrackListAction(string mediaId, out List<mashin.Models.MediaItem>? items)
    {
        lock (_cacheLock)
        {
            return _trackListActionCache.TryGetValue(mediaId, out items);
        }
    }

    private void AddTrackListActionItems(List<MediaBrowserCompat.MediaItem> items, string sourceKey, List<Track> tracks)
    {
        if (tracks.Count == 0)
        {
            return;
        }

        var playActionId = BuildId(PrefixTrackListAction, TrackListActionPlay, sourceKey);
        var shuffleActionId = BuildId(PrefixTrackListAction, TrackListActionShuffle, sourceKey);

        CacheTrackListAction(playActionId, tracks.Cast<mashin.Models.MediaItem>().ToList());

        var shuffledTracks = tracks
            .OrderBy(_ => System.Random.Shared.Next())
            .Cast<mashin.Models.MediaItem>()
            .ToList();
        CacheTrackListAction(shuffleActionId, shuffledTracks);

        items.Add(CreatePlayableItem(
            playActionId,
            "Abspielen",
            $"{tracks.Count} Tracks",
            Resource.Drawable.play_arrow));

        items.Add(CreatePlayableItem(
            shuffleActionId,
            "Zufällige Wiedergabe",
            $"{tracks.Count} Tracks",
            Resource.Drawable.shuffle));
    }

    private static bool IsListenBrainzFolder(RecommendationFolder folder)
    {
        return folder != null
            && !string.IsNullOrWhiteSpace(folder.Provider)
            && folder.Provider.StartsWith("listenbrainz_recommendations--", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(folder.ItemId);
    }

    private static RecommendationFolder? FindRecommendationFolderById(IEnumerable<RecommendationFolder> folders, string itemId)
    {
        return folders.FirstOrDefault(folder =>
            string.Equals(folder.ItemId, itemId, StringComparison.OrdinalIgnoreCase));
    }

    private static string NormalizePlaylistDisplayName(string? playlistName)
    {
        var normalizedPlaylistName = playlistName ?? string.Empty;
        return normalizedPlaylistName.StartsWith("Radio: ", StringComparison.OrdinalIgnoreCase)
            ? normalizedPlaylistName[7..].TrimStart()
            : normalizedPlaylistName;
    }

    #endregion

    #region MediaSession Callback

    private sealed class MediaSessionCallback : MediaSessionCompat.Callback
    {
        private readonly MediaBrowserService _service;

        public MediaSessionCallback(MediaBrowserService service)
        {
            _service = service;
        }

        public override void OnPlay()
        {
            var playback = _service._playbackService;
            if (playback == null || playback.PlaybackState.State == PlayerStateType.Playing)
            {
                return;
            }

            _ = Task.Run(async () =>
            {
                try
                {
                    await playback.TogglePlayPauseAsync();
                }
                catch
                {
                }
            });
        }

        public override void OnPause()
        {
            var playback = _service._playbackService;
            if (playback == null || playback.PlaybackState.State != PlayerStateType.Playing)
            {
                return;
            }

            _ = Task.Run(async () =>
            {
                try
                {
                    await playback.TogglePlayPauseAsync();
                }
                catch
                {
                }
            });
        }

        public override void OnSkipToNext()
        {
            var playback = _service._playbackService;
            if (playback == null)
            {
                return;
            }

            _ = playback.NextTrackAsync().ContinueWith(_ => { }, TaskContinuationOptions.OnlyOnFaulted);
        }

        public override void OnSkipToPrevious()
        {
            var playback = _service._playbackService;
            if (playback == null)
            {
                return;
            }

            _ = playback.PreviousTrackAsync().ContinueWith(_ => { }, TaskContinuationOptions.OnlyOnFaulted);
        }

        public override void OnStop()
        {
            var playback = _service._playbackService;
            if (playback == null || playback.PlaybackState.State != PlayerStateType.Playing)
            {
                return;
            }

            _ = playback.TogglePlayPauseAsync().ContinueWith(_ => { }, TaskContinuationOptions.OnlyOnFaulted);
        }

        public override void OnSkipToQueueItem(long id)
        {
            var playback = _service._playbackService;
            if (playback == null)
            {
                return;
            }

            if (id < 0 || id > int.MaxValue)
            {
                return;
            }

            _ = playback.PlayQueueIndexAsync((int)id).ContinueWith(_ => { }, TaskContinuationOptions.OnlyOnFaulted);
        }

        public override void OnCustomAction(string? action, Bundle? extras)
        {
            if (string.IsNullOrWhiteSpace(action))
            {
                return;
            }

            var playback = _service._playbackService;
            if (playback == null)
            {
                return;
            }

            _ = Task.Run(async () =>
            {
                try
                {
                    if (string.Equals(action, CustomActionToggleShuffle, StringComparison.Ordinal))
                    {
                        await playback.ToggleShuffleAsync(playback.ShuffleEnabled);
                        _service.SyncMediaSessionState();
                        return;
                    }

                    if (string.Equals(action, CustomActionToggleFavorite, StringComparison.Ordinal))
                    {
                        await ToggleCurrentTrackFavoriteAsync(playback, _service._userDataService);
                        _service.SyncMediaSessionState();
                        return;
                    }

                    if (string.Equals(action, CustomActionToggleRepeatMode, StringComparison.Ordinal))
                    {
                        await playback.ToggleRepeatModeAsync(playback.RepeatMode);
                        _service.SyncMediaSessionState();
                        return;
                    }

                }
                catch
                {
                }
            });
        }

        public override void OnPlayFromMediaId(string? mediaId, Bundle? extras)
        {
            if (string.IsNullOrWhiteSpace(mediaId))
            {
                return;
            }

            var playback = _service._playbackService;
            if (playback == null)
            {
                return;
            }

            _ = Task.Run(async () =>
            {
                try
                {
                    if (_service.TryGetCachedTrackListAction(mediaId, out var actionTracks)
                        && actionTracks != null
                        && actionTracks.Count > 0)
                    {
                        await playback.PlayMediaAsync(actionTracks);
                        return;
                    }

                    if (_service.TryGetCachedMediaItem(mediaId, out var cached) && cached != null)
                    {
                        if (cached is Track track)
                        {
                            await PlayTrackWithRadioSeedAsync(playback, track);
                            return;
                        }

                        if (cached is Playlist playlist)
                        {
                            var tracks = await _service._musicAssistantService?.GetPlaylistTracksAsync(playlist.ItemId, playlist.Provider)!;
                            if (tracks != null && tracks.Count > 0)
                            {
                                await playback.PlayMediaAsync(tracks.Cast<mashin.Models.MediaItem>().ToList());
                            }
                            else
                            {
                                await playback.PlayMediaAsync(new List<mashin.Models.MediaItem> { playlist });
                            }

                            return;
                        }

                        if (cached is Album album)
                        {
                            var albumTracks = await _service._musicAssistantService?.GetAlbumTracksAsync(album.ItemId, album.Provider)!;
                            if (albumTracks != null && albumTracks.Count > 0)
                            {
                                await playback.PlayMediaAsync(albumTracks.Cast<mashin.Models.MediaItem>().ToList());
                            }

                            return;
                        }
                    }

                    if (TryParseId(mediaId, out var type, out var provider, out var itemId))
                    {
                        if (string.Equals(type, PrefixArtistRadio, StringComparison.Ordinal))
                        {
                            var musicAssistant = _service._musicAssistantService;
                            if (musicAssistant == null)
                            {
                                return;
                            }

                            var resolvedArtist = await musicAssistant.GetArtistAsync(itemId, provider)
                                ?? new Artist
                                {
                                    ItemId = itemId,
                                    Provider = provider,
                                    Name = "Artist"
                                };

                            await PlayArtistRadioAsync(playback, musicAssistant, resolvedArtist);
                            return;
                        }

                        if (string.Equals(type, PrefixTrack, StringComparison.Ordinal))
                        {
                            var resolvedTrack = await _service._musicAssistantService?.GetTrackAsync(itemId, provider)!;
                            if (resolvedTrack != null)
                            {
                                await PlayTrackWithRadioSeedAsync(playback, resolvedTrack);
                            }
                        }

                        if (string.Equals(type, PrefixAlbum, StringComparison.Ordinal))
                        {
                            var resolvedAlbumTracks = await _service._musicAssistantService?.GetAlbumTracksAsync(itemId, provider)!;
                            if (resolvedAlbumTracks != null && resolvedAlbumTracks.Count > 0)
                            {
                                await playback.PlayMediaAsync(resolvedAlbumTracks.Cast<mashin.Models.MediaItem>().ToList());
                            }
                        }
                    }
                }
                catch
                {
                }
            });
        }

        private static async Task PlayTrackWithRadioSeedAsync(PlaybackService playback, Track track)
        {
            var targetItems = new List<mashin.Models.MediaItem> { track };

            await playback.PlayMediaAsync(targetItems);
            await playback.PlayMediaRadioNextAsync(targetItems);

            const int duplicateIndex = 1;
            string? duplicateQueueItemId = null;
            for (var attempt = 0; attempt < 10; attempt++)
            {
                if (playback.CurrentQueueItems.Count > duplicateIndex)
                {
                    duplicateQueueItemId = playback.CurrentQueueItems[duplicateIndex].QueueItemId;
                    if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
                    {
                        break;
                    }
                }

                await Task.Delay(500);
            }

            if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
            {
                await playback.DeleteQueueItemAsync(duplicateQueueItemId);
            }
        }

        private static async Task PlayArtistRadioAsync(
            PlaybackService playback,
            MusicAssistantService? musicAssistant,
            Artist artist)
        {
            if (musicAssistant == null
                || string.IsNullOrWhiteSpace(artist.ItemId)
                || string.IsNullOrWhiteSpace(artist.Provider))
            {
                return;
            }

            var topTracks = await musicAssistant.GetArtistTopTracksAsync(artist.ItemId, artist.Provider);
            if (topTracks.Count == 0)
            {
                return;
            }

            var randomTopTrack = topTracks[System.Random.Shared.Next(topTracks.Count)];
            await playback.PlayMediaAsync(new List<mashin.Models.MediaItem> { randomTopTrack });
            await playback.PlayMediaRadioNextAsync(new List<mashin.Models.MediaItem> { artist });
        }

        private static async Task ToggleCurrentTrackFavoriteAsync(
            PlaybackService playback,
            UserDataService? userDataService)
        {
            var currentTrack = playback.CurrentQueueItem?.MediaItem;
            if (currentTrack == null || userDataService == null || string.IsNullOrWhiteSpace(currentTrack.Uri))
            {
                return;
            }

            var targetFavoriteState = !currentTrack.Favorite;
            await userDataService.SetFavoriteAsync(new[] { currentTrack }, targetFavoriteState);
        }

        public override void OnPlayFromSearch(string? query, Bundle? extras)
        {
            var playback = _service._playbackService;
            var musicAssistant = _service._musicAssistantService;
            if (playback == null || musicAssistant == null)
            {
                return;
            }

            _ = Task.Run(async () =>
            {
                try
                {
                    var textQuery = query?.Trim() ?? string.Empty;
                    if (string.IsNullOrWhiteSpace(textQuery))
                    {
                        var fallback = await musicAssistant.GetLibraryTracksAsync(limit: 1, orderBy: "sort_name");
                        var firstFallback = fallback.FirstOrDefault();
                        if (firstFallback != null)
                        {
                            await playback.PlayMediaAsync(new List<mashin.Models.MediaItem> { firstFallback });
                        }

                        return;
                    }

                    var result = await musicAssistant.SearchAsync(
                        searchQuery: textQuery,
                        mediaTypes: new[] { MediaType.Track, MediaType.Playlist, MediaType.Album },
                        limit: 25,
                        libraryOnly: false);

                    var firstTrack = result?.Tracks?.FirstOrDefault();
                    if (firstTrack != null)
                    {
                        await playback.PlayMediaAsync(new List<mashin.Models.MediaItem> { firstTrack });
                        return;
                    }

                    var firstPlaylist = result?.Playlists?.FirstOrDefault();
                    if (firstPlaylist != null)
                    {
                        var tracks = await musicAssistant.GetPlaylistTracksAsync(firstPlaylist.ItemId, firstPlaylist.Provider);
                        if (tracks.Count > 0)
                        {
                            await playback.PlayMediaAsync(tracks.Cast<mashin.Models.MediaItem>().ToList());
                        }
                    }
                }
                catch
                {
                }
            });
        }
    }

    #endregion
}
