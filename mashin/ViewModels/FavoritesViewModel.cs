using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
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
    private readonly IUserDataService _userDataService;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<FavoritesViewModel> _logger;

    private ObservableRangeCollection<Track> _tracks = new();
    private ObservableRangeCollection<Album> _albums = new();
    private ObservableRangeCollection<Playlist> _playlists = new();
    private ObservableRangeCollection<Artist> _artists = new();
    private ObservableRangeCollection<ContextMenuItem> _contextMenuItems = new();

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

    public IMediaItemActions MediaActions { get; }

    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand PlaylistTappedCommand { get; }
    public ICommand ShowContextMenuAtAnchorCommand { get; }
    public ICommand ShowContextMenuAtPositionCommand { get; }

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
        IUserDataService userDataService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<FavoritesViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));
        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));
        PlaylistTappedCommand = new Command<Playlist>(async playlist => await _navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));

        ShowContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_contextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_contextMenuItems, anchor);
            }
        });

        ShowContextMenuAtPositionCommand = new Command<Point>(async position =>
        {
            if (_contextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_contextMenuItems, position);
            }
        });
    }

    #endregion

    #region INavigationAware

    public Task OnNavigatedToAsync(object? parameter)
    {
        _logger.LogInformation("Loading favorites");
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
            var snapshot = await _userDataService.GetFavoritesSnapshotAsync() ?? new FavoritesSnapshot();

            var tasks = new Task[]
            {
                LoadTracksAsync(snapshot),
                LoadAlbumsAsync(snapshot),
                LoadPlaylistsAsync(snapshot),
                LoadArtistsAsync(snapshot)
            };

            await Task.WhenAll(tasks);

            await BuildContextMenuAsync();

            _logger.LogInformation("Favorites loaded");
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
                .Select(BuildTrackFromSnapshot)
                .ToList();

            await _musicAssistant.EnrichWithProviderInfoAsync(tracks);

            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = i + 1;
                tracks[i].Favorite = true;
            }

            Tracks = new ObservableRangeCollection<Track>(tracks);
            IsLoadingTracks = false;
            await Task.Delay(50);
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
                .Select(BuildAlbumFromSnapshot)
                .ToList();

            await _musicAssistant.EnrichWithProviderInfoAsync(favoriteAlbums);
            Albums = new ObservableRangeCollection<Album>(favoriteAlbums);
            IsLoadingAlbums = false;
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
                .Select(BuildPlaylistFromSnapshot)
                .ToList();

            await _musicAssistant.EnrichWithProviderInfoAsync(favoritePlaylists);
            Playlists = new ObservableRangeCollection<Playlist>(favoritePlaylists);
            IsLoadingPlaylists = false;
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
                .Select(BuildArtistFromSnapshot)
                .ToList();

            await _musicAssistant.EnrichWithProviderInfoAsync(favoriteArtists);
            Artists = new ObservableRangeCollection<Artist>(favoriteArtists);
            IsLoadingArtists = false;
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

    private async Task BuildContextMenuAsync()
    {
        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await PlaySelectedTracksWithModesAsync(Tracks))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaNextAsync(Tracks.Where(t => t.IsSelected)))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaLastAsync(Tracks.Where(t => t.IsSelected)))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufuegen",
                Icon = FluentIcons.Add12,
                SubItems = await GetPlaylistSubItemsAsync()
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(Tracks.Where(t => t.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentIcons.Add12,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(Tracks.Where(t => t.IsSelected)))
            }
        };

        _contextMenuItems = menu;
    }

    private async Task PlaySelectedTracksWithModesAsync(IEnumerable<Track> tracks)
    {
        var trackList = tracks.ToList();
        var selectedTracks = trackList.Where(t => t.IsSelected).ToList();
        if (selectedTracks.Count == 0)
        {
            return;
        }

        // multiple tracks selected:  play the first one and queue the rest selected tracks
        if (selectedTracks.Count > 1)
        {
            await MediaActions.PlayMediaAsync(selectedTracks.First());

            var remainingTracks = selectedTracks.Skip(1).ToList();
            if (remainingTracks.Count > 0)
            {
                await MediaActions.PlayMediaNextAsync(remainingTracks);
            }

            return;
        }

        // single track selected: play track and queue remaining tracks in cyclic order
        var selectedTrack = selectedTracks.First();
        await MediaActions.PlayMediaAsync(selectedTrack);

        var selectedIndex = trackList.IndexOf(selectedTrack);
        if (selectedIndex < 0 || trackList.Count <= 1)
        {
            return;
        }

        var itemsToQueue = new List<Track>(trackList.Count - 1);
        var trailingCount = trackList.Count - selectedIndex - 1;

        if (trailingCount > 0)
        {
            itemsToQueue.AddRange(trackList.GetRange(selectedIndex + 1, trailingCount));
        }

        if (selectedIndex > 0)
        {
            itemsToQueue.AddRange(trackList.GetRange(0, selectedIndex));
        }

        if (itemsToQueue.Count > 0)
        {
            await MediaActions.PlayMediaNextAsync(itemsToQueue);
        }
    }

    private async Task<ObservableRangeCollection<ContextMenuItem>> GetPlaylistSubItemsAsync()
    {
        var items = new ObservableRangeCollection<ContextMenuItem>();

        try
        {
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(orderBy: "sort_name");
            ApplyPlaylistDisplayNames(playlists);

            foreach (var playlist in playlists)
            {
                if (playlist.Name.StartsWith("~", StringComparison.Ordinal))
                {
                    continue;
                }

                items.Add(new ContextMenuItem
                {
                    Text = playlist.DisplayName,
                    Icon = FluentIcons.Add12,
                    Command = new Command(async () =>
                        await MediaActions.AddToPlaylistAsync(Tracks.Where(t => t.IsSelected), playlist))
                });
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to build playlist subitems for favorites");
        }

        return items;
    }

    #endregion

    #region Helpers

    private void ApplyPlaylistDisplayNames(IEnumerable<Playlist> playlists)
    {
        var prefix = GetUserPlaylistPrefix();

        foreach (var playlist in playlists)
        {
            playlist.DisplayName = playlist.Name;

            if (!string.IsNullOrWhiteSpace(prefix)
                && !string.IsNullOrWhiteSpace(playlist.Name)
                && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            {
                playlist.DisplayName = playlist.Name[prefix.Length..];
            }
        }
    }

    private static Track BuildTrackFromSnapshot(FavoriteTrackSnapshot snapshot)
    {
        var track = new Track
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Duration = snapshot.Duration,
            Favorite = true,
            ProviderMappings = GetProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            track.DisplayName = snapshot.DisplayName;
        }

        track.Metadata = BuildMetadata(snapshot.ImageUrl);

        if (snapshot.Album != null)
        {
            var album = new Album
            {
                ItemId = snapshot.Album.ItemId,
                Provider = snapshot.Album.Provider,
                Name = snapshot.Album.Name,
                Year = snapshot.Album.Year,
                Favorite = false,
                ProviderMappings = GetProviderMappings(snapshot.Album.ProviderMappings)
            };

            album.DisplayName = snapshot.Album.Name;
            album.Metadata = BuildMetadata(snapshot.Album.ImageUrl);
            track.Album = album;
        }

        if (snapshot.Artists.Count > 0)
        {
            track.Artists = snapshot.Artists
                .Select(BuildArtistFromRef)
                .ToList();
        }

        return track;
    }

    private static Album BuildAlbumFromSnapshot(FavoriteAlbumSnapshot snapshot)
    {
        var album = new Album
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Year = snapshot.Year,
            Favorite = true,
            ProviderMappings = GetProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            album.DisplayName = snapshot.DisplayName;
        }

        album.Metadata = BuildMetadata(snapshot.ImageUrl);

        if (snapshot.Artists.Count > 0)
        {
            album.Artists = snapshot.Artists
                .Select(BuildArtistFromRef)
                .ToList();
        }

        return album;
    }

    private static Playlist BuildPlaylistFromSnapshot(FavoritePlaylistSnapshot snapshot)
    {
        var playlist = new Playlist
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Favorite = true,
            ProviderMappings = GetProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            playlist.DisplayName = snapshot.DisplayName;
        }

        playlist.Metadata = BuildMetadata(snapshot.ImageUrl);
        return playlist;
    }

    private static Artist BuildArtistFromSnapshot(FavoriteArtistSnapshot snapshot)
    {
        var artist = new Artist
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Favorite = true,
            ProviderMappings = GetProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            artist.DisplayName = snapshot.DisplayName;
        }

        artist.Metadata = BuildMetadata(snapshot.ImageUrl);
        return artist;
    }

    private static Artist BuildArtistFromRef(FavoriteArtistRef snapshot)
    {
        var artist = new Artist
        {
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            ProviderMappings = GetProviderMappings(snapshot.ProviderMappings)
        };

        artist.DisplayName = snapshot.Name;
        return artist;
    }

    private static MediaItemMetadata? BuildMetadata(string? imageUrl)
    {
        if (string.IsNullOrWhiteSpace(imageUrl))
        {
            return null;
        }

        return new MediaItemMetadata
        {
            Images = new List<MediaItemImage>
            {
                new()
                {
                    Path = imageUrl,
                    Provider = string.Empty,
                    RemotelyAccessible = true
                }
            }
        };
    }

    private static List<ProviderMapping> GetProviderMappings(List<ProviderMapping>? snapshotMappings)
    {
        if (snapshotMappings == null || snapshotMappings.Count == 0)
        {
            return new List<ProviderMapping>();
        }

        return snapshotMappings
            .Select(mapping => new ProviderMapping
            {
                ItemId = mapping.ItemId,
                ProviderDomain = mapping.ProviderDomain,
                ProviderInstance = mapping.ProviderInstance,
                Available = mapping.Available,
                Url = mapping.Url
            })
            .ToList();
    }

    private string? GetUserPlaylistPrefix()
    {
        var username = _userDataService.CurrentUser?.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
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

        _contextMenuItems.Clear();

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
