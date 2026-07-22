using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.Controls;
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
    private readonly IUserDataService _userDataService;
    private readonly SettingsService _settings;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<AlbumDetailViewModel> _logger;
    private readonly Random _shuffleRandom = new();

    private Album? _album;
    private ObservableRangeCollection<Track> _tracks = new();
    private ObservableRangeCollection<Album> _otherAlbums = new();
    private ObservableRangeCollection<Artist> _similarArtists = new();
    private ObservableRangeCollection<ContextMenuItem> _headerContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _trackContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _albumContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();

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
                OnPropertyChanged(nameof(ImageUri));
                OnPropertyChanged(nameof(AlbumYearText));
                OnPropertyChanged(nameof(AlbumDescription));
                OnPropertyChanged(nameof(HasDescription));
                OnPropertyChanged(nameof(IsAlbumFavorite));
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

    public string? ImageUri => Album?.ImageUri;

    public string AlbumYearText => Album?.Year?.ToString() ?? string.Empty;

    public string? AlbumDescription => Album?.Metadata?.Description;

    public bool HasDescription => !string.IsNullOrWhiteSpace(Album?.Metadata?.Description);

    public bool IsAlbumFavorite => Album?.Favorite ?? false;

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

    public IMediaItemActions MediaActions { get; }
    public PlaybackService PlaybackService { get; }

    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand ShowHeaderContextMenuAtAnchorCommand { get; }
    public ICommand ShowHeaderContextMenuAtPositionCommand { get; }
    public ICommand ShowTrackContextMenuAtAnchorCommand { get; }
    public ICommand ShowTrackContextMenuAtPositionCommand { get; }
    public ICommand ShowAlbumContextMenuAtAnchorCommand { get; }
    public ICommand ShowAlbumContextMenuAtPositionCommand { get; }
    public ICommand ShowArtistContextMenuAtAnchorCommand { get; }
    public ICommand ShowArtistContextMenuAtPositionCommand { get; }
    public ICommand PlayTracksCommand { get; }
    public ICommand ShuffleTracksCommand { get; }
    public ICommand PlayOtherAlbumsCommand { get; }
    public ICommand ShuffleOtherAlbumsCommand { get; }
    public ICommand PlaySimilarArtistsCommand { get; }
    public ICommand ShuffleSimilarArtistsCommand { get; }
    public ICommand PlayAlbumCommand { get; }
    public ICommand ShuffleAlbumCommand { get; }
    public ICommand ToggleAlbumFavoriteCommand { get; }
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
        IUserDataService userDataService,
        SettingsService settings,
        IMediaItemActions mediaActions,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<AlbumDetailViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _settings = settings;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;
        PlaybackService = playbackService;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

        PlayAlbumCommand = new Command(async () =>
        {
            if (Album != null)
            {
                await PlaybackService.PlayMediaAsync(new List<MediaItem> { Album });
            }
        });

        ShuffleAlbumCommand = new Command(async () =>
        {
            if (Album == null)
            {
                return;
            }

            var tracks = Tracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.ShufflePlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        PlayTracksCommand = new Command(async () =>
        {
            var tracks = Tracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { tracks[0] });

            var remainingTracks = tracks.Skip(1).ToList();
            if (remainingTracks.Count > 0)
            {
                await PlaybackService.PlayMediaNextAsync(remainingTracks.Cast<MediaItem>().ToList());
            }
        });

        ShuffleTracksCommand = new Command(async () =>
        {
            if (Album == null)
            {
                return;
            }

            var tracks = Tracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.ShufflePlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        PlayOtherAlbumsCommand = new Command(async () =>
        {
            var albums = OtherAlbums.ToList();
            if (albums.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { albums[0] });
        });

        ShuffleOtherAlbumsCommand = new Command(async () =>
        {
            var albums = OtherAlbums.ToList();
            if (albums.Count == 0)
            {
                return;
            }

            var randomIndex = _shuffleRandom.Next(albums.Count);
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { albums[randomIndex] });
        });

        PlaySimilarArtistsCommand = new Command(async () =>
        {
            var similarArtists = SimilarArtists.ToList();
            if (similarArtists.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { similarArtists[0] });
        });

        ShuffleSimilarArtistsCommand = new Command(async () =>
        {
            var similarArtists = SimilarArtists.ToList();
            if (similarArtists.Count == 0)
            {
                return;
            }

            var randomIndex = _shuffleRandom.Next(similarArtists.Count);
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { similarArtists[randomIndex] });
        });

        ToggleAlbumFavoriteCommand = new Command(async () =>
        {
            if (Album == null)
            {
                return;
            }

            if (Album.Favorite)
            {
                await MediaActions.RemoveFromFavoritesAsync(Album);
            }
            else
            {
                await MediaActions.AddToFavoritesAsync(Album);
            }

            OnPropertyChanged(nameof(IsAlbumFavorite));
            await BuildHeaderContextMenuAsync();
        });

        ToggleDescriptionCommand = new Command(() => IsDescriptionExpanded = !IsDescriptionExpanded);

        ShowHeaderContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (_headerContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_headerContextMenuItems, anchor);
            }
        });

        ShowHeaderContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (_headerContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_headerContextMenuItems, position);
            }
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
   
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load album: {AlbumId}", albumId);
        }
    }

    private async Task LoadAlbumMetadataAsync(string albumId, string provider)
    {
        IsLoadingMetadata = true;
        try
        {
            Album = await _musicAssistant.GetAlbumAsync(albumId, provider);
            if (Album != null)
            {
                Album.Favorite = await _userDataService.IsFavoriteAsync(Album);
                OnPropertyChanged(nameof(IsAlbumFavorite));
            }

            if (Album == null)
            {
                _logger.LogWarning("Album not found: {AlbumId}", albumId);
            }

            _ = BuildHeaderContextMenuAsync();

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
                tracks[i].Index = tracks[i].TrackNumber > 0 ? tracks[i].TrackNumber - 1 : i;
                if (Album != null)
                {
                    tracks[i].Album = Album;
                }

                processedTracks.Add(tracks[i]);
            }

            Tracks = new ObservableRangeCollection<Track>(processedTracks);
            IsLoadingTracks = false;
            await Task.Delay(50);
            
            _ = BuildTrackContextMenuAsync();

            // Set fallback artists
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

            OtherAlbums = new ObservableRangeCollection<Album>(filteredAlbums);
            IsLoadingOtherAlbums = false;

            _ = BuildAlbumContextMenuAsync();
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
            
            _ = BuildArtistContextMenuAsync();

        }
        finally
        {
            IsLoadingSimilarArtists = false;
        }
    }

    #endregion

    #region Context Menu

    private Task BuildHeaderContextMenuAsync()
    {
        if (Album == null)
        {
            _headerContextMenuItems = new ObservableRangeCollection<ContextMenuItem>();
            return Task.CompletedTask;
        }

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Als NÃ¤chstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () => await PlaybackService.PlayMediaNextAsync(new List<MediaItem> { Album }))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () => await PlaybackService.PlayMediaLastAsync(new List<MediaItem> { Album }))
            },
            new() { IsSeparator = true }
        };

        if (Album.Favorite)
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    await MediaActions.RemoveFromFavoritesAsync(Album);
                    OnPropertyChanged(nameof(IsAlbumFavorite));
                    await BuildHeaderContextMenuAsync();
                })
            });
        }
        else
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Zu Favoriten hinzufÃ¼gen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                {
                    await MediaActions.AddToFavoritesAsync(Album);
                    OnPropertyChanged(nameof(IsAlbumFavorite));
                    await BuildHeaderContextMenuAsync();
                })
            });
        }

        _headerContextMenuItems = menu;
        return Task.CompletedTask;
    }

    private async Task BuildTrackContextMenuAsync()
    {
        var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(orderBy: "sort_name");
        ApplyPlaylistDisplayNames(playlists);

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaySelectedTracksWithModesAsync(Tracks))
            },
            new()
            {
                Text = "Als NÃ¤chstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaNextAsync(Tracks.Where(t => t.IsSelected).Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaLastAsync(Tracks.Where(t => t.IsSelected).Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufÃ¼gen",
                Icon = FluentIcons.Add12,
                SubItems = new ObservableCollection<ContextMenuItem>(
                    playlists
                        .Where(playlist => !playlist.Name.StartsWith("~"))
                        .Select(playlist => new ContextMenuItem
                        {
                            Text = playlist.DisplayName,
                            Icon = FluentIcons.TextBulletListLtr16,
                            Command = new Command(async () =>
                                await MediaActions.AddToPlaylistAsync(
                                    Tracks.Where(t => t.IsSelected),
                                    playlist))
                        }))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufÃ¼gen",
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
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { selectedTracks.First() });

            var remainingTracks = selectedTracks.Skip(1).ToList();
            if (remainingTracks.Count > 0)
            {
                await PlaybackService.PlayMediaNextAsync(remainingTracks.Cast<MediaItem>().ToList());
            }

            return;
        }

        // single track selected: play track and queue remaining tracks in cyclic order
        var selectedTrack = selectedTracks.First();
        await PlaybackService.PlayMediaAsync(new List<MediaItem> { selectedTrack });

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
            await PlaybackService.PlayMediaNextAsync(itemsToQueue.Cast<MediaItem>().ToList());
        }
    }

    private Task BuildAlbumContextMenuAsync()
    {
        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaAsync(OtherAlbums.Where(a => a.IsSelected).Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als NÃ¤chstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaNextAsync(OtherAlbums.Where(a => a.IsSelected).Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaLastAsync(OtherAlbums.Where(a => a.IsSelected).Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufÃ¼gen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(OtherAlbums.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(OtherAlbums.Where(a => a.IsSelected)))
            }
        };

        _albumContextMenuItems = menu;
        return Task.CompletedTask;
    }

    private Task BuildArtistContextMenuAsync()
    {
        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaAsync(SimilarArtists.Where(a => a.IsSelected).Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als NÃ¤chstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaNextAsync(SimilarArtists.Where(a => a.IsSelected).Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaLastAsync(SimilarArtists.Where(a => a.IsSelected).Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufÃ¼gen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(SimilarArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(SimilarArtists.Where(a => a.IsSelected)))
            }
        };

        _artistContextMenuItems = menu;
        return Task.CompletedTask;
    }

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

    private string? GetUserPlaylistPrefix()
    {
        var username = _settings.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    protected void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        if (propertyName == nameof(IsLoadingMetadata)
            || propertyName == nameof(IsLoadingTracks))
        {
            _navigationService.IsNavigating = IsLoadingMetadata || IsLoadingTracks;
        }
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
        _headerContextMenuItems.Clear();
        _trackContextMenuItems.Clear();
        _albumContextMenuItems.Clear();
        _artistContextMenuItems.Clear();
        PropertyChanged = null;
    }

    #endregion
}

