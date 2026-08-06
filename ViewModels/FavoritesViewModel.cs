using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.Controls;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using System.Windows.Input;

namespace mashin.ViewModels;

public sealed class FavoritesViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly UserDataService _userDataService;
    private readonly SettingsService _settings;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<FavoritesViewModel> _logger;
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

    public UserDataService UserDataService => _userDataService;
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

    public FavoritesViewModel(
        MusicAssistantService musicAssistant,
        UserDataService userDataService,
        SettingsService settings,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<FavoritesViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _settings = settings;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

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

        ShowTrackContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_trackContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_trackContextMenuItems, anchor);
            }
        });

        ShowTrackContextMenuAtPositionCommand = new Command<Point>(async position =>
        {
            if (_trackContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_trackContextMenuItems, position);
            }
        });

        ShowAlbumContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_albumContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_albumContextMenuItems, anchor);
            }
        });

        ShowAlbumContextMenuAtPositionCommand = new Command<Point>(async position =>
        {
            if (_albumContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_albumContextMenuItems, position);
            }
        });

        ShowPlaylistContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_playlistContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, anchor);
            }
        });

        ShowPlaylistContextMenuAtPositionCommand = new Command<Point>(async position =>
        {
            if (_playlistContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, position);
            }
        });

        ShowArtistContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_artistContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_artistContextMenuItems, anchor);
            }
        });

        ShowArtistContextMenuAtPositionCommand = new Command<Point>(async position =>
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
        _logger.LogDebug("Loading favorites");
        _ = LoadFavoritesAsync();
        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from favorites");
        return Task.CompletedTask;
    }

    #endregion

    #region Favorites Loading

    private async Task LoadFavoritesAsync()
    {
        IsLoadingTracks = true;
        IsLoadingAlbums = true;
        IsLoadingPlaylists = true;
        IsLoadingArtists = true;

        try
        {
            var snapshot = await _userDataService.GetFavoritesAsync();

            var tasks = new Task[]
            {
                LoadTracksAsync(snapshot),
                LoadAlbumsAsync(snapshot),
                LoadPlaylistsAsync(snapshot),
                LoadArtistsAsync(snapshot)
            };

            await Task.WhenAll(tasks);

            _logger.LogDebug("Favorites loaded");
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Loading favorites completed with errors");
        }
    }

    private async Task LoadTracksAsync(FavoritesSnapshot snapshot)
    {
        try
        {
            var tracks = snapshot.Tracks
                .Select(track => UserDataSnapshotMapper.ToTrack(track, favorite: true))
                .ToList();

            if (tracks.Count > 0)
            {
                await _musicAssistant.EnrichWithProviderInfoAsync(tracks);
            }

            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = i;
                tracks[i].Favorite = true;
            }

            Tracks = new ObservableRangeCollection<Track>(tracks);
            IsLoadingTracks = false;
            await Task.Delay(50);

            _ = BuildTrackContextMenuAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load favorite tracks");
        }
        finally
        {
            IsLoadingTracks = false;
        }
    }

    private async Task LoadAlbumsAsync(FavoritesSnapshot snapshot)
    {
        try
        {
            var favoriteAlbums = snapshot.Albums
                .Select(album => UserDataSnapshotMapper.ToAlbum(album, favorite: true))
                .ToList();

            if (favoriteAlbums.Count > 0)
            {
                await _musicAssistant.EnrichWithProviderInfoAsync(favoriteAlbums);
            }

            Albums = new ObservableRangeCollection<Album>(favoriteAlbums);
            IsLoadingAlbums = false;

            _ = BuildAlbumContextMenuAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load favorite albums");
        }
        finally
        {
            IsLoadingAlbums = false;
        }
    }

    private async Task LoadPlaylistsAsync(FavoritesSnapshot snapshot)
    {
        try
        {
            var favoritePlaylists = snapshot.Playlists
                .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist, favorite: true))
                .ToList();

            if (favoritePlaylists.Count > 0)
            {
                await _musicAssistant.EnrichWithProviderInfoAsync(favoritePlaylists);
            }

            Playlists = new ObservableRangeCollection<Playlist>(favoritePlaylists);
            IsLoadingPlaylists = false;

            _ = BuildPlaylistContextMenuAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load favorite playlists");
        }
        finally
        {
            IsLoadingPlaylists = false;
        }
    }

    private async Task LoadArtistsAsync(FavoritesSnapshot snapshot)
    {
        try
        {
            var favoriteArtists = snapshot.Artists
                .Select(artist => UserDataSnapshotMapper.ToArtist(artist, favorite: true))
                .ToList();

            if (favoriteArtists.Count > 0)
            {
                await _musicAssistant.EnrichWithProviderInfoAsync(favoriteArtists);
            }

            Artists = new ObservableRangeCollection<Artist>(favoriteArtists);
            IsLoadingArtists = false;

            _ = BuildArtistContextMenuAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load favorite artists");
        }
        finally
        {
            IsLoadingArtists = false;
        }
    }

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
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var targetTracks = Tracks
                        .Where(track => track.IsSelected
                            && !string.IsNullOrWhiteSpace(track.ItemId)
                            && !string.IsNullOrWhiteSpace(track.Provider))
                        .DistinctBy(track => string.Concat(track.Provider, "|", track.ItemId))
                        .ToList();

                    if (targetTracks.Count == 0)
                    {
                        _logger.LogDebug("Cannot start track radio: no target tracks available.");
                        return;
                    }

                    var targetItems = targetTracks.Cast<MediaItem>().ToList();
                    await PlaybackService.PlayMediaAsync(targetItems);
                    await PlaybackService.PlayMediaRadioNextAsync(targetItems);

                    var duplicateIndex = targetTracks.Count;
                    string? duplicateQueueItemId = null;
                    for (var attempt = 0; attempt < 10; attempt++)
                    {
                        if (PlaybackService.CurrentQueueItems.Count > duplicateIndex)
                        {
                            duplicateQueueItemId = PlaybackService.CurrentQueueItems[duplicateIndex].QueueItemId;
                            if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
                            {
                                break;
                            }
                        }

                        await Task.Delay(500);
                    }

                    if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
                    {
                        await PlaybackService.DeleteQueueItemAsync(duplicateQueueItemId);
                    }
                    else
                    {
                        _logger.LogDebug("Cannot remove queue index {QueueIndex} after starting track radio: no queue item id available.", duplicateIndex);
                    }
                })
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
                {
                    var selectedMediaItems = Tracks.Where(t => t.IsSelected).Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = Tracks.Where(t => t.IsSelected).Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
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
                {
                    var selectedMediaItems = Albums.Where(a => a.IsSelected).Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = Albums.Where(a => a.IsSelected).Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
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
                {
                    var selectedMediaItems = Playlists.Where(p => p.IsSelected).Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = Playlists.Where(p => p.IsSelected).Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
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
                        _logger.LogDebug("Cannot start artist radio: no target artists available.");
                        return;
                    }

                    var radioArtist = targetArtists[_shuffleRandom.Next(targetArtists.Count)];
                    var topTracks = await _musicAssistant.GetArtistTopTracksAsync(radioArtist.ItemId, radioArtist.Provider);
                    if (topTracks.Count == 0)
                    {
                        _logger.LogDebug("Cannot start artist radio: no top tracks available for selected artist {ArtistId}.", radioArtist.ItemId);
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
                {
                    var selectedMediaItems = GetSelectedArtists().Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedArtists().Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
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
        var snapshot = await _userDataService.GetPlaylistsAsync();
        var playlists = snapshot.Playlists
            .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist))
            .ToList();

        foreach (var playlist in playlists)
        {
            items.Add(new ContextMenuItem
            {
                Text = playlist.DisplayName,
                Icon = FluentIcons.TextBulletListLtr16,
                Command = new Command(async () =>
                    await _userDataService.AddPlaylistTracksAsync(playlist.ItemId, Tracks.Where(t => t.IsSelected).ToList()))
            });
        }

        return items;
    }

    #endregion

    #region Helpers

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

        if (propertyName == nameof(IsLoadingTracks) && !IsLoadingTracks)
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

