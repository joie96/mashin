using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using Microsoft.Extensions.Logging;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Windows.Input;
using mashin.Collections;

namespace mashin.ViewModels;

public class AlbumDetailViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<AlbumDetailViewModel> _logger;

    private Album? _album;
    private ObservableRangeCollection<Track> _tracks = new();
    private ObservableRangeCollection<Album> _otherAlbums = new();
    private ObservableRangeCollection<Artist> _similarArtists = new();
    private ObservableRangeCollection<ContextMenuItem> _contextMenuItems = new();

    private readonly IReadOnlyList<TableViewSkeleton> _trackSkeletons = Enumerable.Range(0, 10)
        .Select(_ => new TableViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _otherAlbumSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _similarArtistSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();

    private bool _isLoadingMetadata;
    private bool _isLoadingTracks;
    private bool _isLoadingOtherAlbums;
    private bool _isLoadingSimilarArtists;
    private bool _isDescriptionExpanded;
    private bool _disposed;

    #endregion

    #region Properties

    public Album? Album
    {
        get => _album;
        set
        {
            if (SetProperty(ref _album, value))
            {
                OnPropertyChanged(nameof(AlbumName));
                OnPropertyChanged(nameof(ArtistName));
                OnPropertyChanged(nameof(ImageUrl));
                OnPropertyChanged(nameof(AlbumYearText));
                OnPropertyChanged(nameof(AlbumDescription));
                OnPropertyChanged(nameof(HasDescription));
                IsDescriptionExpanded = false;
            }
        }
    }

    public string AlbumName => Album?.Name ?? "Unbekanntes Album";

    public string ArtistName
    {
        get
        {
            var artistName = Album?.Artists?.FirstOrDefault()?.Name;
            if (!string.IsNullOrWhiteSpace(artistName))
            {
                return artistName;
            }

            var trackArtist = Tracks.FirstOrDefault()?.Artists?.FirstOrDefault()?.Name;
            return trackArtist ?? "Unbekannter Interpret";
        }
    }

    public string? ImageUrl => Album?.ImageUrl;

    public string AlbumYearText => Album?.Year?.ToString() ?? string.Empty;

    public string? AlbumDescription => Album?.Metadata?.Description;

    public bool HasDescription => !string.IsNullOrWhiteSpace(Album?.Metadata?.Description);

    public ObservableRangeCollection<Track> Tracks
    {
        get => _tracks;
        set
        {
            if (SetProperty(ref _tracks, value))
            {
                OnPropertyChanged(nameof(TrackItems));
            }
        }
    }

    public ObservableRangeCollection<Album> OtherAlbums
    {
        get => _otherAlbums;
        set
        {
            if (SetProperty(ref _otherAlbums, value))
            {
                OnPropertyChanged(nameof(OtherAlbumItems));
            }
        }
    }

    public ObservableRangeCollection<Artist> SimilarArtists
    {
        get => _similarArtists;
        set
        {
            if (SetProperty(ref _similarArtists, value))
            {
                OnPropertyChanged(nameof(SimilarArtistItems));
            }
        }
    }

    public ObservableRangeCollection<ContextMenuItem> ContextMenuItems
    {
        get => _contextMenuItems;
        set => SetProperty(ref _contextMenuItems, value);
    }

    public IMediaItemActions MediaActions { get; }

    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand ShowContextMenuAtAnchorCommand { get; }
    public ICommand ShowContextMenuAtPositionCommand { get; }
    public ICommand ToggleDescriptionCommand { get; }

    public bool IsLoadingMetadata
    {
        get => _isLoadingMetadata;
        set => SetProperty(ref _isLoadingMetadata, value);
    }

    public bool IsLoadingTracks
    {
        get => _isLoadingTracks;
        set
        {
            if (SetProperty(ref _isLoadingTracks, value))
            {
                OnPropertyChanged(nameof(TrackItems));
            }
        }
    }

    public bool IsLoadingOtherAlbums
    {
        get => _isLoadingOtherAlbums;
        set
        {
            if (SetProperty(ref _isLoadingOtherAlbums, value))
            {
                OnPropertyChanged(nameof(OtherAlbumItems));
            }
        }
    }

    public bool IsLoadingSimilarArtists
    {
        get => _isLoadingSimilarArtists;
        set
        {
            if (SetProperty(ref _isLoadingSimilarArtists, value))
            {
                OnPropertyChanged(nameof(SimilarArtistItems));
            }
        }
    }

    public bool IsDescriptionExpanded
    {
        get => _isDescriptionExpanded;
        set
        {
            if (SetProperty(ref _isDescriptionExpanded, value))
            {
                OnPropertyChanged(nameof(DescriptionMaxLines));
            }
        }
    }

    public int DescriptionMaxLines => IsDescriptionExpanded ? int.MaxValue : 4;

    public IEnumerable<object> TrackItems => IsLoadingTracks ? _trackSkeletons : _tracks;
    public IEnumerable<object> OtherAlbumItems => IsLoadingOtherAlbums ? _otherAlbumSkeletons : _otherAlbums;
    public IEnumerable<object> SimilarArtistItems => IsLoadingSimilarArtists ? _similarArtistSkeletons : _similarArtists;

    #endregion

    #region Construction

    public AlbumDetailViewModel(
        MusicAssistantService musicAssistant,
        IPlayerService playerService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<AlbumDetailViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

        ToggleDescriptionCommand = new Command(() => IsDescriptionExpanded = !IsDescriptionExpanded);

        ShowContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (ContextMenuItems?.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(ContextMenuItems, anchor);
            }
        });

        ShowContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (ContextMenuItems?.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(ContextMenuItems, position);
            }
        });
    }

    #endregion

    #region INavigationAware

    public Task OnNavigatedToAsync(object? parameter)
    {
        if (parameter is MediaItem item)
        {
            _logger.LogInformation("Navigated to album target: {ItemId} ({Provider})", item.ItemId, item.Provider);
            _ = LoadAlbumAsync(item.ItemId, item.Provider);
        }
        else
        {
            _logger.LogWarning("NavigatedTo called without valid MediaItem parameter");
        }

        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from album: {AlbumName}", AlbumName);
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    public async Task LoadAlbumAsync(string albumId, string providerInstanceOrDomain = "library")
    {
        IsLoadingMetadata = true;
        IsLoadingTracks = true;
        IsLoadingOtherAlbums = true;
        IsLoadingSimilarArtists = true;

        try
        {
            await LoadAlbumMetadataAsync(albumId, providerInstanceOrDomain);
            
            var tracksTask = LoadAlbumTracksAsync(albumId, providerInstanceOrDomain);
            var otherAlbumsTask = LoadOtherAlbumsAsync();

            await Task.WhenAll(tracksTask, otherAlbumsTask);

            await LoadSimilarArtistsAsync();
            await BuildContextMenuAsync();
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load album: {AlbumId}", albumId);
            _navigationService.IsNavigating = false;
        }
    }

    private async Task LoadAlbumMetadataAsync(string albumId, string provider)
    {
        IsLoadingMetadata = true;
        try
        {
            Album = await _musicAssistant.GetAlbumAsync(albumId, provider);

            if (Album == null)
            {
                _logger.LogWarning("Album not found: {AlbumId}", albumId);
            }
        }
        finally
        {
            IsLoadingMetadata = false;
        }
    }

    private async Task LoadAlbumTracksAsync(string albumId, string provider)
    {
        IsLoadingTracks = true;
        try
        {
            var tracks = await _musicAssistant.GetAlbumTracksAsync(albumId, provider);
            var processedTracks = new List<Track>();

            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = tracks[i].TrackNumber > 0 ? tracks[i].TrackNumber : i + 1;
                if (Album != null)
                {
                    tracks[i].Album = Album;
                }

                processedTracks.Add(tracks[i]);
            }

            Tracks = new ObservableRangeCollection<Track>(processedTracks);

            if (Album != null && (Album.Artists == null || Album.Artists.Count == 0))
            {
                var fallbackArtists = Tracks.FirstOrDefault()?.Artists;
                if (fallbackArtists != null && fallbackArtists.Count > 0)
                {
                    Album.Artists = fallbackArtists;
                    OnPropertyChanged(nameof(ArtistName));
                }
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load album tracks");
        }
        finally
        {
            IsLoadingTracks = false;
            _navigationService.IsNavigating = false;
        }
    }

    private async Task LoadOtherAlbumsAsync()
    {
        IsLoadingOtherAlbums = true;
        try
        {
            var artist = Album?.Artists?.FirstOrDefault() ?? Tracks.FirstOrDefault()?.Artists?.FirstOrDefault();
            if (artist == null)
            {
                OtherAlbums = new ObservableRangeCollection<Album>();
                return;
            }

            var albums = await _musicAssistant.GetArtistAlbumsAsync(artist.ItemId, artist.Provider);
            var filteredAlbums = albums
                .Where(a => a.ItemId != Album?.ItemId)
                .OrderByDescending(a => a.Year ?? 0)
                .ToList();

            // Render first 10 albums "slowly"
            var visibleAlbums = filteredAlbums.Take(10);
            foreach (var album in visibleAlbums)
            {
                OtherAlbums.Add(album);
                await Task.Delay(10);
            }

            // Add remaining albums quickly (virtuallized in collection view)
            var remainingAlbums = filteredAlbums.Skip(10).ToList();
            if (remainingAlbums.Count > 0)
            {
                // Add remaining albums in batches of 20
                foreach (var batch in remainingAlbums.Chunk(20))
                {
                    OtherAlbums.AddRange(batch);
                    await Task.Delay(30); // Minimal delay between batches
                }
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load other albums");
        }
        finally
        {
            IsLoadingOtherAlbums = false;
        }
    }

    private async Task LoadSimilarArtistsAsync()
    {
        IsLoadingSimilarArtists = true;
        try
        {
            var albumTrack = Tracks.FirstOrDefault();
            var similarTracks = albumTrack != null
                ? await _musicAssistant.GetSimilarTracksAsync(
                    albumTrack.ItemId,
                    albumTrack.Provider,
                    limit: 50,
                    allowLookup: true)
                : new List<Track>();

            if (albumTrack != null && similarTracks.Count == 0)
            {
                var versions = await _musicAssistant.GetTrackVersionsAsync(albumTrack.ItemId, albumTrack.Provider);
                var fallbackVersion = versions
                    .FirstOrDefault(v => !string.Equals(v.Provider, albumTrack.Provider, StringComparison.OrdinalIgnoreCase))
                    ?? versions.FirstOrDefault();

                if (fallbackVersion != null)
                {
                    similarTracks = await _musicAssistant.GetSimilarTracksAsync(
                        fallbackVersion.ItemId,
                        fallbackVersion.Provider,
                        limit: 50,
                        allowLookup: true);
                }
            }

            var albumArtistId = Album?.Artists?.FirstOrDefault()?.ItemId;

            var uniqueArtists = similarTracks
                .SelectMany(track => track.Artists ?? Enumerable.Empty<Artist>())
                .GroupBy(artist => artist.ItemId)
                .Select(group => group.First())
                .Where(artist => artist.ItemId != albumArtistId)
                .Take(15)
                .ToList();

            SimilarArtists = new ObservableRangeCollection<Artist>();
            foreach (var artistRef in uniqueArtists)
            {
                try
                {
                    var fullArtist = await _musicAssistant.GetArtistAsync(artistRef.ItemId, artistRef.Provider);
                    if (fullArtist != null)
                    {
                        SimilarArtists.Add(fullArtist);
                    }
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(ex, "Failed to load details for artist: {ArtistId}", artistRef.ItemId);
                }
            }
        }
        finally
        {
            IsLoadingSimilarArtists = false;
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
                    await MediaActions.PlayMediaAsync(Tracks.Where(t => t.IsSelected)))
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

        ContextMenuItems = menu;
    }

    private async Task<ObservableRangeCollection<ContextMenuItem>> GetPlaylistSubItemsAsync()
    {
        var items = new ObservableRangeCollection<ContextMenuItem>();

        try
        {
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(orderBy: "sort_name");

            foreach (var playlist in playlists)
            {
                if (playlist.Name.StartsWith("~")) { continue; }

                items.Add(new ContextMenuItem
                {
                    Text = playlist.Name,
                    Icon = FluentIcons.Add12,
                    Command = new Command(async () =>
                        await MediaActions.AddToPlaylistAsync(
                            Tracks.Where(t => t.IsSelected),
                            playlist))
                });
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load playlists for context menu");
        }

        return items;
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    protected void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    protected bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
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

    #region IDisposable

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        _tracks.Clear();
        _otherAlbums.Clear();
        _similarArtists.Clear();
        _contextMenuItems.Clear();
        PropertyChanged = null;
    }

    #endregion
}
