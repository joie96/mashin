using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
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

    private readonly MusicAssistantService _musicAssistant;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<SearchViewModel> _logger;

    private List<Track> _allTracks = new();
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
    private bool _isTracksExpanded;
    private bool _isLoadingTracks;
    private bool _isLoadingAlbums;
    private bool _isLoadingPlaylists;
    private bool _isLoadingArtists;
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
                OnPropertyChanged(nameof(HasMoreTracks));
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
    public bool HasMoreTracks => _allTracks.Count > 10;
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

    public bool IsTracksExpanded
    {
        get => _isTracksExpanded;
        set
        {
            if (SetProperty(ref _isTracksExpanded, value))
            {
                UpdateDisplayedTracks();
            }
        }
    }

    public IMediaItemActions MediaActions { get; }

    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand PlaylistTappedCommand { get; }
    public ICommand ShowContextMenuAtAnchorCommand { get; }
    public ICommand ShowContextMenuAtPositionCommand { get; }
    public ICommand ToggleTracksCommand { get; }

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
        IPlayerService playerService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<SearchViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));
        PlaylistTappedCommand = new Command<Playlist>(async playlist => await _navigationService.NavigateToAsync<PlaylistDetailPage>(playlist));
        ToggleTracksCommand = new Command(() => IsTracksExpanded = !IsTracksExpanded);

        ShowContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (_contextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_contextMenuItems, anchor);
            }
        });

        ShowContextMenuAtPositionCommand = new Command<Point>(async (position) =>
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
        if (parameter is SearchRequest request)
        {
            _logger.LogInformation("Search started");
            _ = SearchAsync(request);
        }
        else
        {
            _logger.LogWarning("NavigatedTo called without SearchRequest");
        }

        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from search results");
        return Task.CompletedTask;
    }

    #endregion

    #region Track Display

    private void UpdateDisplayedTracks()
    {
        if (_allTracks.Count == 0)
        {
            if (Tracks.Count > 0)
            {
                Tracks.Clear();
            }
            return;
        }

        if (IsTracksExpanded)
        {

            var missingTracks = _allTracks.Skip(Tracks.Count).ToList();
            if (missingTracks.Count > 0)
            {
                Tracks.AddRange(missingTracks);
            }
            
        }
        else
        {
            var desiredCount = Math.Min(10, _allTracks.Count);
            var extraTracks = Tracks.Skip(desiredCount).ToList();
            if (extraTracks.Count > 0)
            {
                Tracks.RemoveRange(extraTracks, NotifyCollectionChangedAction.Remove);
            }  
        }
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

            // Context Menu
            await BuildContextMenuAsync();

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
                _allTracks = results.Tracks ?? new List<Track>();
                for (var i = 0; i < _allTracks.Count; i++)
                {
                    _allTracks[i].Index = i + 1;
                }

                IsTracksExpanded = false;

                var visibleTracks = _allTracks.Take(10).ToList();

                Tracks = new ObservableRangeCollection<Track>(visibleTracks);
                IsLoadingTracks = false;
                await Task.Delay(50);

                OnPropertyChanged(nameof(HasMoreTracks));
                break;

            case MediaType.Album:
                var albums = results.Albums ?? new List<Album>();
                
                var visibleAlbums = albums.Take(10);
               
                Albums = new ObservableRangeCollection<Album>(visibleAlbums);
                IsLoadingAlbums = false;
                await Task.Delay(50);
                
                var remainingAlbums = albums.Skip(visibleAlbums.Count()).ToList();
                if (remainingAlbums.Count > 0)
                {
                    foreach (var batch in remainingAlbums.Chunk(10))
                    {
                        Albums.AddRange(batch);       
                        await Task.Delay(50);
                    }
                }
                break;

            case MediaType.Playlist:
                var playlists = results.Playlists ?? new List<Playlist>();
                
                var visiblePlaylists = playlists.Take(10);

                Playlists = new ObservableRangeCollection<Playlist>(visiblePlaylists);
                IsLoadingPlaylists = false;
                await Task.Delay(50);
                
                var remainingPlaylists = playlists.Skip(visiblePlaylists.Count()).ToList();
                if (remainingPlaylists.Count > 0)
                {
                    foreach (var batch in remainingPlaylists.Chunk(10))
                    {
                        Playlists.AddRange(batch);
                        await Task.Delay(50);
                    }
                }
                
                break;

            case MediaType.Artist:
                var artists = results.Artists ?? new List<Artist>();

                var visibleArtists = artists.Take(10);

                Artists = new ObservableRangeCollection<Artist>(visibleArtists);
                IsLoadingArtists = false;
                await Task.Delay(50);
                
                var remainingArtists = artists.Skip(visibleArtists.Count()).ToList();
                if (remainingArtists.Count > 0)
                {
                    foreach (var batch in remainingArtists.Chunk(10))
                    {
                        await Task.Delay(50);
                        Artists.AddRange(batch);
                    }
                }            
                break;
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
                    await PlaySelectedTracksWithModesAsync(Tracks, playbackContextItem: null))
            },
            new()
            {
                Text = "Als Nächstes spielen",
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
                Text = "Zu Wiedergabeliste hinzufügen",
                Icon = FluentIcons.Add12,
                SubItems = await GetPlaylistSubItemsAsync()
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
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

    private async Task PlaySelectedTracksWithModesAsync(IEnumerable<Track> tracks, MediaItem? playbackContextItem)
    {
        var selectedTracks = tracks.Where(t => t.IsSelected).ToList();
        if (selectedTracks.Count == 0)
        {
            return;
        }

        if (selectedTracks.Count > 1 || playbackContextItem == null)
        {
            await MediaActions.PlayMediaAsync(selectedTracks.First());

            var remainingTracks = selectedTracks.Skip(1).ToList();
            if (remainingTracks.Count > 0)
            {
                await MediaActions.PlayMediaNextAsync(remainingTracks);
            }

            return;
        }

        await MediaActions.PlayMediaAsync(playbackContextItem, selectedTracks.First());
    }

    private async Task<ObservableRangeCollection<ContextMenuItem>> GetPlaylistSubItemsAsync()
    {
        var items = new ObservableRangeCollection<ContextMenuItem>();

        try
        {
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(orderBy: "sort_name");

            foreach (var playlist in playlists)
            {
                if (playlist.Name.StartsWith("~"))
                {
                    continue;
                }

                items.Add(new ContextMenuItem
                {
                    Text = playlist.Name,
                    Icon = FluentIcons.Add12,
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

        _contextMenuItems.Clear();

        _albums.Clear();
        _artists.Clear();
        _playlists.Clear();
        _tracks.Clear();
        _allTracks.Clear();

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
