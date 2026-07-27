using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using mashin.Collections;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.ApplicationModel;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Windows.Input;

namespace mashin.ViewModels;

public sealed record SearchRequest(string Query, MediaType[] MediaTypes);

public class SearchViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    #region Cache State

    private sealed record CachedSearchState(
        IReadOnlyList<Track> Tracks,
        IReadOnlyList<Album> Albums,
        IReadOnlyList<Playlist> Playlists,
        IReadOnlyList<Artist> Artists,
        bool HasSearchRequest);

    private static CachedSearchState? s_cachedSearchState;

    #endregion

    private readonly MusicAssistantService _musicAssistant;
    private readonly SettingsService _settings;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<SearchViewModel> _logger;
    private readonly Random _shuffleRandom = new();

    private ObservableRangeCollection<Track> _tracks = new();
    private ObservableRangeCollection<Album> _albums = new();
    private ObservableRangeCollection<Playlist> _playlists = new();
    private ObservableRangeCollection<Artist> _artists = new();
    private ObservableRangeCollection<ContextMenuItem> _trackContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _albumContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _playlistContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();
    private readonly IReadOnlyList<TableViewSkeleton> _trackSkeletons = Enumerable.Range(0, 10)
        .Select(_ => new TableViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _albumSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _playlistSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _artistSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();
    private bool _isLoadingTracks;
    private bool _isLoadingAlbums;
    private bool _isLoadingPlaylists;
    private bool _isLoadingArtists;
    private bool _hasSearchRequest;
    private MediaType? _navigationAnchorType;
    private bool _disposed;

    #endregion

    #region Properties

    public ObservableRangeCollection<Track> Tracks
    {
        get => _tracks;
        private set
        {
            value ??= new ObservableRangeCollection<Track>();
            if (_tracks != null)
            {
                _tracks.CollectionChanged -= OnTracksCollectionChanged;
            }
            if (SetProperty(ref _tracks, value))
            {
                if (_tracks != null)
                {
                    _tracks.CollectionChanged += OnTracksCollectionChanged;
                }
                OnPropertyChanged(nameof(HasTracks));
                OnPropertyChanged(nameof(HasResults));
                OnPropertyChanged(nameof(ShowNoTracksMessage));
                OnPropertyChanged(nameof(ShowTrackTable));
                OnPropertyChanged(nameof(TrackItems));
            }
        }
    }

    public ObservableRangeCollection<Album> Albums
    {
        get => _albums;
        private set
        {
            value ??= new ObservableRangeCollection<Album>();
            if (_albums != null)
            {
                _albums.CollectionChanged -= OnAlbumsCollectionChanged;
            }
            if (SetProperty(ref _albums, value))
            {
                if (_albums != null)
                {
                    _albums.CollectionChanged += OnAlbumsCollectionChanged;
                }
                OnPropertyChanged(nameof(HasAlbums));
                OnPropertyChanged(nameof(HasResults));
                OnPropertyChanged(nameof(ShowNoAlbumsMessage));
                OnPropertyChanged(nameof(ShowAlbumsRowView));
                OnPropertyChanged(nameof(AlbumItems));
            }
        }
    }

    public ObservableRangeCollection<Playlist> Playlists
    {
        get => _playlists;
        private set
        {
            value ??= new ObservableRangeCollection<Playlist>();
            if (_playlists != null)
            {
                _playlists.CollectionChanged -= OnPlaylistsCollectionChanged;
            }
            if (SetProperty(ref _playlists, value))
            {
                if (_playlists != null)
                {
                    _playlists.CollectionChanged += OnPlaylistsCollectionChanged;
                }
                OnPropertyChanged(nameof(HasPlaylists));
                OnPropertyChanged(nameof(HasResults));
                OnPropertyChanged(nameof(ShowNoPlaylistsMessage));
                OnPropertyChanged(nameof(ShowPlaylistsRowView));
                OnPropertyChanged(nameof(PlaylistItems));
            }
        }
    }

    public ObservableRangeCollection<Artist> Artists
    {
        get => _artists;
        private set
        {
            value ??= new ObservableRangeCollection<Artist>();
            if (_artists != null)
            {
                _artists.CollectionChanged -= OnArtistsCollectionChanged;
            }
            if (SetProperty(ref _artists, value))
            {
                if (_artists != null)
                {
                    _artists.CollectionChanged += OnArtistsCollectionChanged;
                }
                OnPropertyChanged(nameof(HasArtists));
                OnPropertyChanged(nameof(HasResults));
                OnPropertyChanged(nameof(ShowNoArtistsMessage));
                OnPropertyChanged(nameof(ShowArtistsRowView));
                OnPropertyChanged(nameof(ArtistItems));
            }
        }
    }

    public bool HasTracks => Tracks.Count > 0;
    public bool HasAlbums => Albums.Count > 0;
    public bool HasPlaylists => Playlists.Count > 0;
    public bool HasArtists => Artists.Count > 0;
    public bool HasResults => HasTracks || HasAlbums || HasPlaylists || HasArtists;
    public bool ShowTrackTable => IsLoadingTracks || HasTracks;
    public bool ShowAlbumsRowView => IsLoadingAlbums || HasAlbums;
    public bool ShowPlaylistsRowView => IsLoadingPlaylists || HasPlaylists;
    public bool ShowArtistsRowView => IsLoadingArtists || HasArtists;

    public IEnumerable<object> TrackItems => IsLoadingTracks ? _trackSkeletons : _tracks;
    public IEnumerable<object> AlbumItems => IsLoadingAlbums ? _albumSkeletons : _albums;
    public IEnumerable<object> PlaylistItems => IsLoadingPlaylists ? _playlistSkeletons : _playlists;
    public IEnumerable<object> ArtistItems => IsLoadingArtists ? _artistSkeletons : _artists;

    public bool IsLoadingTracks
    {
        get => _isLoadingTracks;
        private set
        {
            if (SetProperty(ref _isLoadingTracks, value))
            {
                OnPropertyChanged(nameof(ShowNoTracksMessage));
                OnPropertyChanged(nameof(ShowTrackTable));
                OnPropertyChanged(nameof(TrackItems));
            }
        }
    }

    public bool IsLoadingAlbums
    {
        get => _isLoadingAlbums;
        private set
        {
            if (SetProperty(ref _isLoadingAlbums, value))
            {
                OnPropertyChanged(nameof(ShowNoAlbumsMessage));
                OnPropertyChanged(nameof(ShowAlbumsRowView));
                OnPropertyChanged(nameof(AlbumItems));
            }
        }
    }

    public bool IsLoadingPlaylists
    {
        get => _isLoadingPlaylists;
        private set
        {
            if (SetProperty(ref _isLoadingPlaylists, value))
            {
                OnPropertyChanged(nameof(ShowNoPlaylistsMessage));
                OnPropertyChanged(nameof(ShowPlaylistsRowView));
                OnPropertyChanged(nameof(PlaylistItems));
            }
        }
    }

    public bool IsLoadingArtists
    {
        get => _isLoadingArtists;
        private set
        {
            if (SetProperty(ref _isLoadingArtists, value))
            {
                OnPropertyChanged(nameof(ShowNoArtistsMessage));
                OnPropertyChanged(nameof(ShowArtistsRowView));
                OnPropertyChanged(nameof(ArtistItems));
            }
        }
    }

    public bool ShowNoTracksMessage => !IsLoadingTracks && !HasTracks;
    public bool ShowNoAlbumsMessage => !IsLoadingAlbums && !HasAlbums;
    public bool ShowNoPlaylistsMessage => !IsLoadingPlaylists && !HasPlaylists;
    public bool ShowNoArtistsMessage => !IsLoadingArtists && !HasArtists;

    public bool HasSearchRequest
    {
        get => _hasSearchRequest;
        private set => SetProperty(ref _hasSearchRequest, value);
    }

    public IMediaItemActions MediaActions { get; }
    public PlaybackService PlaybackService { get; }

    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand PlaylistTappedCommand { get; }
    public ICommand ShowTrackContextMenuAtAnchorCommand { get; }
    public ICommand ShowTrackContextMenuAtPositionCommand { get; }
    public ICommand ShowAlbumContextMenuAtAnchorCommand { get; }
    public ICommand ShowAlbumContextMenuAtPositionCommand { get; }
    public ICommand ShowPlaylistContextMenuAtAnchorCommand { get; }
    public ICommand ShowPlaylistContextMenuAtPositionCommand { get; }
    public ICommand ShowArtistContextMenuAtAnchorCommand { get; }
    public ICommand ShowArtistContextMenuAtPositionCommand { get; }
    public ICommand PlayTracksCommand { get; }
    public ICommand ShuffleTracksCommand { get; }
    public ICommand PlayAlbumsCommand { get; }
    public ICommand ShuffleAlbumsCommand { get; }
    public ICommand PlayPlaylistsCommand { get; }
    public ICommand ShufflePlaylistsCommand { get; }
    public ICommand PlayArtistsCommand { get; }
    public ICommand ShuffleArtistsCommand { get; }

    #endregion

    #region Collection Changed Handlers

    private void OnTracksCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasTracks));
        OnPropertyChanged(nameof(HasResults));
        OnPropertyChanged(nameof(ShowNoTracksMessage));
        OnPropertyChanged(nameof(ShowTrackTable));
        OnPropertyChanged(nameof(TrackItems));
    }

    private void OnAlbumsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasAlbums));
        OnPropertyChanged(nameof(HasResults));
        OnPropertyChanged(nameof(ShowNoAlbumsMessage));
        OnPropertyChanged(nameof(ShowAlbumsRowView));
        OnPropertyChanged(nameof(AlbumItems));
    }

    private void OnPlaylistsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasPlaylists));
        OnPropertyChanged(nameof(HasResults));
        OnPropertyChanged(nameof(ShowNoPlaylistsMessage));
        OnPropertyChanged(nameof(ShowPlaylistsRowView));
        OnPropertyChanged(nameof(PlaylistItems));
    }

    private void OnArtistsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasArtists));
        OnPropertyChanged(nameof(HasResults));
        OnPropertyChanged(nameof(ShowNoArtistsMessage));
        OnPropertyChanged(nameof(ShowArtistsRowView));
        OnPropertyChanged(nameof(ArtistItems));
    }

    #endregion

    #region Construction

    public SearchViewModel(
        MusicAssistantService musicAssistant,
        SettingsService settings,
        IMediaItemActions mediaActions,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<SearchViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _settings = settings;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;
        PlaybackService = playbackService;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));
        PlaylistTappedCommand = new Command<Playlist>(async playlist => await _navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));

        PlayTracksCommand = new Command(async () =>
        {
            var tracks = Tracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        ShuffleTracksCommand = new Command(async () =>
        {
            var tracks = Tracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.ShufflePlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        PlayAlbumsCommand = new Command(async () =>
        {
            var albums = Albums.ToList();
            if (albums.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { albums[0] });
        });

        ShuffleAlbumsCommand = new Command(async () =>
        {
            var albums = Albums.ToList();
            if (albums.Count == 0)
            {
                return;
            }

            var randomIndex = _shuffleRandom.Next(albums.Count);
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { albums[randomIndex] });
        });

        PlayPlaylistsCommand = new Command(async () =>
        {
            var playlists = Playlists.ToList();
            if (playlists.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { playlists[0] });
        });

        ShufflePlaylistsCommand = new Command(async () =>
        {
            var playlists = Playlists.ToList();
            if (playlists.Count == 0)
            {
                return;
            }

            var randomIndex = _shuffleRandom.Next(playlists.Count);
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { playlists[randomIndex] });
        });

        PlayArtistsCommand = new Command(async () =>
        {
            var artists = Artists.ToList();
            if (artists.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { artists[0] });
        });

        ShuffleArtistsCommand = new Command(async () =>
        {
            var artists = Artists.ToList();
            if (artists.Count == 0)
            {
                return;
            }

            var randomIndex = _shuffleRandom.Next(artists.Count);
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { artists[randomIndex] });
        });

        ShowTrackContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (_trackContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_trackContextMenuItems, anchor);
            }
        });

        ShowTrackContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (_trackContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_trackContextMenuItems, position);
            }
        });

        ShowAlbumContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (_albumContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_albumContextMenuItems, anchor);
            }
        });

        ShowAlbumContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (_albumContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_albumContextMenuItems, position);
            }
        });

        ShowPlaylistContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (_playlistContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, anchor);
            }
        });

        ShowPlaylistContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (_playlistContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, position);
            }
        });

        ShowArtistContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (_artistContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_artistContextMenuItems, anchor);
            }
        });

        ShowArtistContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (_artistContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_artistContextMenuItems, position);
            }
        });
    }

    #endregion

    #region INavigationAware

    public Task OnNavigatedToAsync(object? parameter)
    {
        if (parameter is SearchRequest request)
        {
            HasSearchRequest = true;
            _logger.LogInformation("Search started");
            _ = SearchAsync(request);
        }
        else
        {
            if (TryRestoreFromCache())
            {
                _logger.LogInformation("Restored cached search results");
            }
            else
            {
                HasSearchRequest = false;
                _logger.LogWarning("NavigatedTo called without SearchRequest");
            }

            _navigationService.IsNavigating = false;
        }

        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from search results");
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    private async Task SearchAsync(SearchRequest request)
    {
        var mediaTypes = request.MediaTypes?.Length > 0
            ? request.MediaTypes
            : new[] { MediaType.Track, MediaType.Album, MediaType.Playlist, MediaType.Artist };

        var requested = new HashSet<MediaType>(mediaTypes);

        _navigationAnchorType = null;
        foreach (var type in new[] { MediaType.Track, MediaType.Album, MediaType.Playlist, MediaType.Artist })
        {
            if (requested.Contains(type))
            {
                _navigationAnchorType = type;
                break;
            }
        }

        IsLoadingTracks = requested.Contains(MediaType.Track);
        IsLoadingAlbums = requested.Contains(MediaType.Album);
        IsLoadingPlaylists = requested.Contains(MediaType.Playlist);
        IsLoadingArtists = requested.Contains(MediaType.Artist);

        var tasks = mediaTypes
            .Distinct()
            .Select(type => SearchForTypeAsync(request.Query, type))
            .ToArray();

        try
        {
            await Task.WhenAll(tasks);

            CacheCurrentState();

            _logger.LogInformation("Search completed");
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Search completed with errors");
        }
    }

    private async Task SearchForTypeAsync(string query, MediaType mediaType)
    {
        try
        {
            var results = await _musicAssistant.SearchAsync(
                query,
                new[] { mediaType },
                limit: 50,
                libraryOnly: false) ?? new SearchResults();

            await ApplyResultsAsync(mediaType, results);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Search failed for media type: {MediaType}", mediaType);
        }
        finally
        {
            switch (mediaType)
            {
                case MediaType.Track:
                    IsLoadingTracks = false;
                    break;
                case MediaType.Album:
                    IsLoadingAlbums = false;
                    break;
                case MediaType.Playlist:
                    IsLoadingPlaylists = false;
                    break;
                case MediaType.Artist:
                    IsLoadingArtists = false;
                    break; 
            }
        }
    }

    private async Task ApplyResultsAsync(MediaType mediaType, SearchResults results)
    {   
        switch (mediaType)
        {
            case MediaType.Track:
                var tracks = results.Tracks ?? new List<Track>();
                for (var i = 0; i < tracks.Count; i++)
                {
                    tracks[i].Index = i;
                }

                Tracks = new ObservableRangeCollection<Track>(tracks);
                IsLoadingTracks = false;
                await Task.Delay(50);

                _ = BuildTrackContextMenuAsync();
                break;

            case MediaType.Album:
                var albums = results.Albums ?? new List<Album>();
                Albums = new ObservableRangeCollection<Album>(albums);
                IsLoadingAlbums = false;

                _ = BuildAlbumContextMenuAsync();
                break;

            case MediaType.Playlist:
                var playlists = results.Playlists ?? new List<Playlist>();
                var prefix = string.Concat("--", _settings.Username);
                playlists = playlists
                    .Where(playlist => !string.IsNullOrWhiteSpace(playlist.Name)
                        && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                    .ToList();

                foreach (var playlist in playlists)
                {
                    playlist.DisplayName = playlist.Name[prefix.Length..];
                }

                Playlists = new ObservableRangeCollection<Playlist>(playlists);
                IsLoadingPlaylists = false;

                _ = BuildPlaylistContextMenuAsync();
                break;

            case MediaType.Artist:
                var artists = results.Artists ?? new List<Artist>();
                Artists = new ObservableRangeCollection<Artist>(artists);
                IsLoadingArtists = false;

                _ = BuildArtistContextMenuAsync();
                break;
        }

        CacheCurrentState();
    }

    #region Cache

    private bool TryRestoreFromCache()
    {
        var cached = s_cachedSearchState;
        if (cached is null || !cached.HasSearchRequest)
        {
            return false;
        }

        HasSearchRequest = true;

        IsLoadingTracks = false;
        IsLoadingAlbums = false;
        IsLoadingPlaylists = false;
        IsLoadingArtists = false;

        Tracks = new ObservableRangeCollection<Track>(cached.Tracks);
        Albums = new ObservableRangeCollection<Album>(cached.Albums);
        Playlists = new ObservableRangeCollection<Playlist>(cached.Playlists);
        Artists = new ObservableRangeCollection<Artist>(cached.Artists);

        _ = BuildTrackContextMenuAsync();
        _ = BuildAlbumContextMenuAsync();
        _ = BuildPlaylistContextMenuAsync();
        _ = BuildArtistContextMenuAsync();

        return true;
    }

    private void CacheCurrentState()
    {
        s_cachedSearchState = new CachedSearchState(
            Tracks.ToList(),
            Albums.ToList(),
            Playlists.ToList(),
            Artists.ToList(),
            HasSearchRequest);
    }

            #endregion

    #endregion

    #region Context Menu

    private async Task BuildTrackContextMenuAsync()
    {
        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaAsync(Tracks.Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = Tracks.Where(t => t.IsSelected).Select(t => (MediaItem)t).ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = Tracks.Where(t => t.IsSelected).Select(t => (MediaItem)t).ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufügen",
                Icon = FluentIcons.Add12,
                SubItems = await GetPlaylistSubItemsAsync()
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(Tracks.Where(t => t.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(Tracks.Where(t => t.IsSelected)))
            }
        };

        _trackContextMenuItems = menu;
    }

    private Task BuildAlbumContextMenuAsync()
    {
        _albumContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                {
                    var items = Albums.Where(a => a.IsSelected).Select(a => (MediaItem)a).ToList();
                    await PlaybackService.PlayMediaAsync(items);
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = Albums.Where(a => a.IsSelected).Select(a => (MediaItem)a).ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = Albums.Where(a => a.IsSelected).Select(a => (MediaItem)a).ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(Albums.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(Albums.Where(a => a.IsSelected)))
            }
        };

        return Task.CompletedTask;
    }

    private Task BuildPlaylistContextMenuAsync()
    {
        _playlistContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                {
                    var items = Playlists.Where(p => p.IsSelected).Select(p => (MediaItem)p).ToList();
                    await PlaybackService.PlayMediaAsync(items);
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = Playlists.Where(p => p.IsSelected).Select(p => (MediaItem)p).ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = Playlists.Where(p => p.IsSelected).Select(p => (MediaItem)p).ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(Playlists.Where(p => p.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(Playlists.Where(p => p.IsSelected)))
            }
        };

        return Task.CompletedTask;
    }

    private Task BuildArtistContextMenuAsync()
    {
        _artistContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                {
                    var items = Artists.Where(a => a.IsSelected).Select(a => (MediaItem)a).ToList();
                    await PlaybackService.PlayMediaAsync(items);
                })
            },
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var targetArtists = GetSelectedArtists();
                    if (targetArtists.Count == 0)
                    {
                        _logger.LogInformation("Cannot start artist radio: no target artists available.");
                        return;
                    }

                    var radioArtist = targetArtists[_shuffleRandom.Next(targetArtists.Count)];
                    var topTracks = await _musicAssistant.GetArtistTopTracksAsync(radioArtist.ItemId, radioArtist.Provider);
                    if (topTracks.Count == 0)
                    {
                        _logger.LogInformation("Cannot start artist radio: no top tracks available for selected artist {ArtistId}.", radioArtist.ItemId);
                        return;
                    }

                    var randomTopTrack = topTracks[_shuffleRandom.Next(topTracks.Count)];
                    await PlaybackService.PlayMediaAsync(new List<MediaItem> { randomTopTrack });
                    await PlaybackService.PlayMediaRadioNextAsync(new List<MediaItem> { radioArtist });
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = Artists.Where(a => a.IsSelected).Select(a => (MediaItem)a).ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = GetSelectedArtists().Select(a => (MediaItem)a).ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(GetSelectedArtists()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(GetSelectedArtists()))
            }
        };

        return Task.CompletedTask;
    }

    private IReadOnlyList<Artist> GetSelectedArtists()
    {
        return Artists.Where(artist => artist.IsSelected).ToList();
    }

    private async Task<ObservableRangeCollection<ContextMenuItem>> GetPlaylistSubItemsAsync()
    {
        var items = new ObservableRangeCollection<ContextMenuItem>();

        try
        {
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(
                orderBy: "sort_name",
                userPrefix: string.Concat("--", _settings.Username));

            foreach (var playlist in playlists)
            {
                items.Add(new ContextMenuItem
                {
                    Text = playlist.DisplayName,
                    Icon = FluentIcons.TextBulletListLtr16,
                    Command = new Command(async () =>
                        await MediaActions.AddToPlaylistAsync(Tracks.Where(t => t.IsSelected), playlist))
                });
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to build playlist subitems for search results");
        }

        return items;
    }

    #endregion

    #region IDisposable

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        _tracks.CollectionChanged -= OnTracksCollectionChanged;
        _albums.CollectionChanged -= OnAlbumsCollectionChanged;
        _playlists.CollectionChanged -= OnPlaylistsCollectionChanged;
        _artists.CollectionChanged -= OnArtistsCollectionChanged;

        _trackContextMenuItems.Clear();
        _albumContextMenuItems.Clear();
        _playlistContextMenuItems.Clear();
        _artistContextMenuItems.Clear();

        _albums.Clear();
        _artists.Clear();
        _playlists.Clear();
        _tracks.Clear();

        PropertyChanged = null;
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        if (_navigationAnchorType == null)
        {
            return;
        }

        if (propertyName == nameof(IsLoadingTracks)
            && _navigationAnchorType == MediaType.Track
            && !IsLoadingTracks)
        {
            _navigationService.IsNavigating = false;
        }
        else if (propertyName == nameof(IsLoadingAlbums)
            && _navigationAnchorType == MediaType.Album
            && !IsLoadingAlbums)
        {
            _navigationService.IsNavigating = false;
        }
        else if (propertyName == nameof(IsLoadingPlaylists)
            && _navigationAnchorType == MediaType.Playlist
            && !IsLoadingPlaylists)
        {
            _navigationService.IsNavigating = false;
        }
        else if (propertyName == nameof(IsLoadingArtists)
            && _navigationAnchorType == MediaType.Artist
            && !IsLoadingArtists)
        {
            _navigationService.IsNavigating = false;
        }
    }

    private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        OnPropertyChanged(propertyName);
        return true;
    }

    #endregion
}

