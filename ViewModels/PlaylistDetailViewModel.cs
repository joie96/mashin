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
using System.Windows.Input;

namespace mashin.ViewModels;

public class PlaylistDetailViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IOverlayService _overlayService;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<PlaylistDetailViewModel> _logger;

    private Playlist? _playlist;
    private ObservableRangeCollection<Track> _tracks = new();
    private ObservableRangeCollection<ContextMenuItem> _headerContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _contentContextMenuItems = new();
    private readonly IReadOnlyList<TableViewSkeleton> _trackSkeletons = Enumerable.Range(0, 10)
        .Select(_ => new TableViewSkeleton())
        .ToList();
    private bool _isLoadingMetadata;
    private bool _isLoadingTracks;
    private bool _isHeaderCollapsed;
    private bool _disposed;
    private Track? _contextMenuTargetTrack;

    #endregion

    #region Properties

    public Playlist? Playlist
    {
        get => _playlist;
        set
        {
            if (SetProperty(ref _playlist, value))
            {
                OnPropertyChanged(nameof(PlaylistName));
                OnPropertyChanged(nameof(ImageUri));
                OnPropertyChanged(nameof(IsPlaylistFavorite));
            }
        }
    }

    public string PlaylistName => Playlist?.DisplayName ?? "Unbekannte Playlist";

    public string? ImageUri => Playlist?.ImageUri;

    public ObservableRangeCollection<Track> Tracks
    {
        get => _tracks;
        set
        {
            if (ReferenceEquals(_tracks, value))
            {
                return;
            }

            _tracks.CollectionChanged -= OnTracksCollectionChanged;
            _tracks = value;
            OnPropertyChanged();

            _tracks.CollectionChanged += OnTracksCollectionChanged;
            OnPropertyChanged(nameof(HasTracks));
            OnPropertyChanged(nameof(ShowTrackTable));
            OnPropertyChanged(nameof(TrackItems));
            OnPropertyChanged(nameof(PlaylistTotalDurationText));
        }
    }


    public bool IsLoadingTracks
    {
        get => _isLoadingTracks;
        private set
        {
            if (SetProperty(ref _isLoadingTracks, value))
            {
                OnPropertyChanged(nameof(ShowTrackTable));
                OnPropertyChanged(nameof(TrackItems));
            }
        }
    }

    public bool IsLoadingMetadata
    {
        get => _isLoadingMetadata;
        private set => SetProperty(ref _isLoadingMetadata, value);
    }

    public bool IsHeaderCollapsed
    {
        get => _isHeaderCollapsed;
        set => SetProperty(ref _isHeaderCollapsed, value);
    }

    public bool HasTracks => Tracks.Count > 0;

    public bool ShowTrackTable => IsLoadingTracks || HasTracks;

    public bool IsPlaylistFavorite => Playlist?.Favorite ?? false;

    public IEnumerable<object> TrackItems => IsLoadingTracks ? _trackSkeletons : _tracks;

    public string PlaylistTotalDurationText
    {
        get
        {
            var totalSeconds = _tracks.Sum(track => Math.Max(0, track.Duration));
            return FormatTotalDuration(totalSeconds);
        }
    }

    public UserDataService UserDataService { get; }
    public PlaybackService PlaybackService { get; }
    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand ShowHeaderContextMenuAtAnchorCommand { get; }
    public ICommand ShowHeaderContextMenuAtPositionCommand { get; }
    public ICommand ShowContentContextMenuAtAnchorCommand { get; }
    public ICommand ShowContentContextMenuAtPositionCommand { get; }
    public ICommand PlayPlaylistCommand { get; }
    public ICommand ShufflePlaylistCommand { get; }
    public ICommand TogglePlaylistFavoriteCommand { get; }
    public ICommand ToggleHeaderCollapsedCommand { get; }

    #endregion

    #region Collection Changed Handlers

    private void OnTracksCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasTracks));
        OnPropertyChanged(nameof(PlaylistTotalDurationText));
        OnPropertyChanged(nameof(ShowTrackTable));
        OnPropertyChanged(nameof(TrackItems));
    }

    #endregion

    #region Construction

    public PlaylistDetailViewModel(
        MusicAssistantService musicAssistant,
        IOverlayService overlayService,
        UserDataService userDataService,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<PlaylistDetailViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _overlayService = overlayService;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        UserDataService = userDataService;
        PlaybackService = playbackService;

        AlbumTappedCommand = new Command<object>(async parameter => 
        { 
            await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter); 
        });

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

        // Context Menu Commands
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

        ShowContentContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (anchor == null)
            {
                return;
            }

            _contextMenuTargetTrack = anchor.BindingContext as Track;
            await BuildContentContextMenuAsync();

            if (_contentContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_contentContextMenuItems, anchor);
            }
        });

        ShowContentContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            await BuildContentContextMenuAsync();

            if (_contentContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_contentContextMenuItems, position);
            }
        });

        PlayPlaylistCommand = new Command(async () =>
        {
            if (Playlist != null)
            {
                await PlaybackService.PlayMediaAsync(new List<MediaItem> { Playlist });
            }
        });

        ShufflePlaylistCommand = new Command(async () =>
        {
            var playlist = Playlist;
            if (playlist == null)
            {
                return;
            }

            if (Tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.ShufflePlayMediaAsync(Tracks.Cast<MediaItem>().ToList());
        });

        TogglePlaylistFavoriteCommand = new Command(async () =>
        {
            if (Playlist == null)
            {
                return;
            }

            var targetFavoriteState = !Playlist.Favorite;
            await UserDataService.SetFavoriteAsync(new[] { Playlist }, targetFavoriteState);

            OnPropertyChanged(nameof(IsPlaylistFavorite));
            _ = BuildHeaderContextMenuAsync();
        });

        ToggleHeaderCollapsedCommand = new Command(() =>
        {
            IsHeaderCollapsed = !IsHeaderCollapsed;
        });
    }

    #endregion

    #region INavigationAware

    public Task OnNavigatedToAsync(object? parameter)
    {
        if (parameter is MediaItem item)
        {
            _logger.LogDebug("Navigated to playlist target: {ItemId} ({Provider})", item.ItemId, item.Provider);
            
            // Load data
            _ = LoadPlaylistAsync(item.ItemId, item.Provider);
        }
        else
        {
            _logger.LogWarning("NavigatedTo called without valid MediaItem parameter");
        }

        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from playlist: {PlaylistName}", PlaylistName);
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    public async Task LoadPlaylistAsync(string playlistId, string providerInstanceOrDomain = "library")
    {
        IsLoadingMetadata = true;
        IsLoadingTracks = true;
        try
        {
            var playlist = await ResolvePlaylistFromServiceAsync(playlistId, providerInstanceOrDomain);
            if (playlist != null)
            {
                await LoadLocalPlaylistAsync(playlist);
                return;
            }

            await LoadPlaylistMetadataAsync(playlistId, providerInstanceOrDomain);
            await LoadPlaylistTracksAsync(playlistId, providerInstanceOrDomain);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load playlist: {PlaylistId}", playlistId);
        }
        finally
        {
            IsLoadingMetadata = false;
            IsLoadingTracks = false;
        }
    }

    private async Task LoadLocalPlaylistAsync(Playlist playlist)
    {
        await _musicAssistant.EnrichWithProviderInfoAsync(new List<Playlist> { playlist });

        Playlist = playlist;
        OnPropertyChanged(nameof(IsPlaylistFavorite));

        var tracks = playlist.Items.ToList();

        if (tracks.Count > 0)
        {
            await _musicAssistant.EnrichWithProviderInfoAsync(tracks);
        }

        for (var i = 0; i < tracks.Count; i++)
        {
            tracks[i].Index = i;
        }

        Tracks = new ObservableRangeCollection<Track>(tracks);
        await BuildHeaderContextMenuAsync();
        await BuildContentContextMenuAsync();

        _logger.LogDebug("Loaded local playlist '{Name}' with {Count} tracks", playlist.Name, tracks.Count);
    }

    private async Task LoadPlaylistMetadataAsync(string playlistId, string providerInstanceOrDomain)
    {
        try
        {
            var playlist = await _musicAssistant.GetPlaylistAsync(playlistId, providerInstanceOrDomain);
            if (playlist != null)
            {
                playlist.Favorite = await UserDataService.IsFavoriteAsync(playlist);
                playlist.DisplayName = playlist.Name;
            }

            Playlist = playlist;
            OnPropertyChanged(nameof(IsPlaylistFavorite));

            if (Playlist == null)
            {
                Tracks = new ObservableRangeCollection<Track>();
                _logger.LogWarning("Playlist not found: {PlaylistId}", playlistId);
                return;
            }

            await BuildHeaderContextMenuAsync();
        }
        finally
        {
            IsLoadingMetadata = false;
        }
    }

    private async Task LoadPlaylistTracksAsync(string playlistId, string providerInstanceOrDomain)
    {
        try
        {
            if (Playlist == null)
            {
                return;
            }

            var tracks = await _musicAssistant.GetPlaylistTracksAsync(
                playlistId,
                providerInstanceOrDomain,
                forceRefresh: true);

            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = i;
                tracks[i].Favorite = await UserDataService.IsFavoriteAsync(tracks[i]);
            }

            Tracks = new ObservableRangeCollection<Track>(tracks);
            await BuildContentContextMenuAsync();

            _logger.LogDebug("Loaded online playlist '{Name}' with {Count} tracks", Playlist.Name, Tracks.Count);
        }
        finally
        {
            IsLoadingTracks = false;
        }
    }

    #endregion

    
    #region Playlist Actions

    private async Task RenamePlaylistAsync()
    {
        var playlist = Playlist!;

        var updatedName = await _overlayService.ShowUpdatePlaylistAsync(playlist);
        if (string.IsNullOrWhiteSpace(updatedName))
        {
            return;
        }

        updatedName = updatedName.Trim();

        var originalName = playlist.Name;
        var originalDisplayName = playlist.DisplayName;

        var renamed = false;
        try
        {
            playlist.Name = updatedName;
            playlist.DisplayName = updatedName;

            await UserDataService.UpdatePlaylistAsync(playlist);
            renamed = true;
        }
        catch (Exception ex)
        {
            playlist.Name = originalName;
            playlist.DisplayName = originalDisplayName;
            _logger.LogError(ex, "Failed to rename playlist: {PlaylistName}", originalName);
        }

        if (renamed)
        {
            OnPropertyChanged(nameof(PlaylistName));
            _ = BuildContentContextMenuAsync();
        }
    }

    private async Task DeletePlaylistAsync()
    {
        var playlist = Playlist!;

        var confirmed = await _overlayService.ShowDeletePlaylistAsync(playlist);
        if (!confirmed)
        {
            return;
        }

        var removed = false;
        try
        {
            await UserDataService.RemovePlaylistAsync(playlist);
            removed = true;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to delete playlist: {PlaylistName}", playlist.Name);
        }

        if (removed)
        {
            await _navigationService.GoBackAsync();
        }
    }

    private async Task SortPlaylistContentAsync()
    {
        var playlist = Playlist;
        if (playlist == null)
        {
            return;
        }

        var sortSelection = await _overlayService.ShowSortContentOverlayAsync();
        if (sortSelection == null)
        {
            return;
        }

        var (sortField, isDescending) = sortSelection.Value;

        IEnumerable<Track> sortedQuery = sortField switch
        {
            "Album" => Tracks
                .OrderBy(track => track.AlbumName, StringComparer.OrdinalIgnoreCase)
                .ThenBy(track => track.DisplayName, StringComparer.OrdinalIgnoreCase)
                .ThenBy(track => track.Index),
            "Artist" => Tracks
                .OrderBy(track => track.ArtistName, StringComparer.OrdinalIgnoreCase)
                .ThenBy(track => track.DisplayName, StringComparer.OrdinalIgnoreCase)
                .ThenBy(track => track.Index),
            _ => Tracks
                .OrderBy(track => track.DisplayName, StringComparer.OrdinalIgnoreCase)
                .ThenBy(track => track.Index)
        };

        if (isDescending)
        {
            sortedQuery = sortedQuery.Reverse();
        }

        var sortedTracks = sortedQuery.ToList();
        var padWidth = Math.Max(2, sortedTracks.Count.ToString().Length);

        for (var i = 0; i < sortedTracks.Count; i++)
        {
            sortedTracks[i].SortName = (i + 1).ToString($"D{padWidth}");
            sortedTracks[i].Index = i;
        }

        Tracks.ReplaceRange(sortedTracks);
        playlist.Items = sortedTracks;

        await UserDataService.UpdatePlaylistAsync(playlist);

        _logger.LogDebug(
            "Playlist sort requested for '{PlaylistName}': field={SortField}, descending={IsDescending}",
            PlaylistName,
            sortField,
            isDescending);
    }

    #endregion

    #region Context Menu

    private Task BuildHeaderContextMenuAsync()
    {
        if (Playlist == null)
        {
            _headerContextMenuItems = new ObservableRangeCollection<ContextMenuItem>();
            return Task.CompletedTask;
        }

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = new List<MediaItem> { Playlist! };
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = new List<MediaItem> { Playlist! };
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true }
        };

        if (Playlist.Favorite)
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    await UserDataService.SetFavoriteAsync(new[] { Playlist }, false);
                    OnPropertyChanged(nameof(IsPlaylistFavorite));
                    _ = BuildHeaderContextMenuAsync();
                })
            });
        }
        else
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                {
                    await UserDataService.SetFavoriteAsync(new[] { Playlist }, true);
                    OnPropertyChanged(nameof(IsPlaylistFavorite));
                    _ = BuildHeaderContextMenuAsync();
                })
            });
        }

        menu.Add(new ContextMenuItem { IsSeparator = true });

        menu.Add(new ContextMenuItem
        {
            Text = "Wiedergabeliste sortieren",
            Icon = FluentIcons.ArrowSort16,
            Command = new Command(async () => await SortPlaylistContentAsync())
        });

        menu.Add(new ContextMenuItem
        {
            Text = "Wiedergabeliste umbenennen",
            Icon = FluentIcons.Rename16,
            Command = new Command(async () => await RenamePlaylistAsync())
        });

        menu.Add(new ContextMenuItem
        {
            Text = "Wiedergabeliste löschen",
            Icon = FluentIcons.Delete12,
            Command = new Command(async () => await DeletePlaylistAsync())
        });

        _headerContextMenuItems = menu;
        return Task.CompletedTask;
    }

    private async Task BuildContentContextMenuAsync()
    {
        var snapshot = await UserDataService.GetPlaylistsAsync();
        var playlists = snapshot.Playlists
            .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist))
            .ToList();

        var targets = GetContextMenuTargetTracks().ToList();
        var isSingleTarget = targets.Count == 1;
        var singleTarget = isSingleTarget ? targets[0] : null;

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                {
                    var tracksForAction = GetContextMenuTargetTracks().ToList();
                    if (tracksForAction.Count == 0)
                    {
                        return;
                    }

                    await PlaybackService.PlayMediaAsync(tracksForAction.Cast<MediaItem>().ToList());
                })
            },
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var tracksForAction = GetContextMenuTargetTracks().ToList();
                    if (tracksForAction.Count == 0)
                    {
                        return;
                    }

                    var targetTracks = tracksForAction
                        .Where(track => !string.IsNullOrWhiteSpace(track.ItemId)
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
                    var items = GetContextMenuTargetTracks().Select(track => (MediaItem)track).ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = GetContextMenuTargetTracks().Select(track => (MediaItem)track).ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            }
        };

        if (isSingleTarget)
        {
            menu.Add(new ContextMenuItem { IsSeparator = true });

            menu.Add(new ContextMenuItem
            {
                Text = "Künstler:inn öffnen",
                Icon = FluentIcons.Person12,
                Command = new Command(async () =>
                {
                    var selectedArtist = GetContextMenuTargetTracks()
                        .SelectMany(track => track.Artists ?? Enumerable.Empty<Artist>())
                        .FirstOrDefault();

                    if (selectedArtist == null)
                    {
                        return;
                    }

                    await _navigationService.NavigateToAsync<ArtistDetailPage>(selectedArtist);
                })
            });

            menu.Add(new ContextMenuItem
            {
                Text = "Album öffnen",
                Icon = FluentIcons.Open16,
                Command = new Command(async () =>
                {
                    var selectedAlbum = GetContextMenuTargetTracks()
                        .Select(track => track.Album)
                        .FirstOrDefault(album => album != null);

                    if (selectedAlbum == null)
                    {
                        return;
                    }

                    await _navigationService.NavigateToAsync<AlbumDetailPage>(selectedAlbum);
                })
            });
        }

        menu.Add(new ContextMenuItem { IsSeparator = true });

        menu.Add(new ContextMenuItem
        {
            Text = "Zu Wiedergabeliste hinzufügen",
            Icon = FluentIcons.Add12,
            SubItems = new ObservableCollection<ContextMenuItem>(
                playlists
                    .Select(playlist => new ContextMenuItem
                    {
                        Text = playlist.DisplayName,
                        Icon = FluentIcons.TextBulletListLtr16,
                        Command = new Command(async () =>
                            await UserDataService.AddPlaylistTracksAsync(
                                playlist.ItemId,
                                GetContextMenuTargetTracks().ToList()))
                    }))
        });

        menu.Add(new ContextMenuItem
        {
            Text = "Aus Wiedergabeliste entfernen",
            Icon = FluentIcons.Subtract12,
            Command = new Command(async () =>
            {
                if (Playlist != null)
                {
                    var tracksToRemove = GetContextMenuTargetTracks().OfType<Track>().ToList();

                    if (!string.IsNullOrWhiteSpace(Playlist.ItemId) && tracksToRemove.Count > 0)
                    {
                        await UserDataService.RemovePlaylistTracksAsync(Playlist.ItemId, tracksToRemove);
                    }

                    // Apply a local delta update so the table does not need a full reset.
                    if (tracksToRemove.Count > 0)
                    {
                        Tracks.RemoveRange(tracksToRemove, NotifyCollectionChangedAction.Remove);

                        for (var i = 0; i < Tracks.Count; i++)
                        {
                            Tracks[i].Index = i;
                        }

                        Playlist.Items = Tracks.ToList();
                        OnPropertyChanged(nameof(PlaylistTotalDurationText));
                    }
                }
            }),
            IsEnabled = true
        });

        menu.Add(new ContextMenuItem { IsSeparator = true });

        if (isSingleTarget)
        {
            if (singleTarget?.Favorite == true)
            {
                menu.Add(new ContextMenuItem
                {
                    Text = "Aus Favoriten entfernen",
                    Icon = FluentFilledIcons.Heart12Filled,
                    IconIsFilled = true,
                    Command = new Command(async () =>
                    {
                        var selectedMediaItems = GetContextMenuTargetTracks().Cast<MediaItem>().ToList();
                        await UserDataService.SetFavoriteAsync(selectedMediaItems, false);
                    })
                });
            }
            else
            {
                menu.Add(new ContextMenuItem
                {
                    Text = "Zu Favoriten hinzufügen",
                    Icon = FluentIcons.Heart12,
                    Command = new Command(async () =>
                    {
                        var selectedMediaItems = GetContextMenuTargetTracks().Cast<MediaItem>().ToList();
                        await UserDataService.SetFavoriteAsync(selectedMediaItems, true);
                    })
                });
            }
        }
        else
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetContextMenuTargetTracks().Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            });

            menu.Add(new ContextMenuItem
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetContextMenuTargetTracks().Cast<MediaItem>().ToList();
                    await UserDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
            });
        }

        _contentContextMenuItems = menu;
    }

    #endregion

    #region Helper Methods
    private async Task<Playlist?> ResolvePlaylistFromServiceAsync(string playlistId, string providerInstanceOrDomain)
    {
        var snapshot = await UserDataService.GetPlaylistsAsync();
        var playlists = snapshot.Playlists
            .Select(playlist => UserDataSnapshotMapper.ToPlaylist(playlist))
            .ToList();

        var byProviderAndId = playlists.FirstOrDefault(playlist =>
            string.Equals(playlist.ItemId, playlistId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(playlist.Provider, providerInstanceOrDomain, StringComparison.OrdinalIgnoreCase));

        if (byProviderAndId != null)
        {
            return byProviderAndId;
        }

        return playlists.FirstOrDefault(playlist =>
            string.Equals(playlist.ItemId, playlistId, StringComparison.OrdinalIgnoreCase));
    }

    private IReadOnlyList<Track> GetContextMenuTargetTracks()
    {
        var selectedTracks = Tracks.Where(track => track.IsSelected).ToList();
        if (selectedTracks.Count > 0)
        {
            return selectedTracks;
        }

        return _contextMenuTargetTrack == null ? Array.Empty<Track>() : new[] { _contextMenuTargetTrack };
    }

    private static string FormatTotalDuration(int totalSeconds)
    {
        if (totalSeconds <= 0)
        {
            return "0m";
        }

        var ts = TimeSpan.FromSeconds(totalSeconds);
        var totalHours = (int)ts.TotalHours;

        if (totalHours > 0)
        {
            return $"{totalHours}h {ts.Minutes}m";
        }

        return $"{Math.Max(1, ts.Minutes)}m";
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
        
        _logger.LogDebug("Disposing PlaylistDetailViewModel for playlist: {PlaylistName}", PlaylistName);
        
        _disposed = true;

        if (_tracks != null)
        {
            _tracks.CollectionChanged -= OnTracksCollectionChanged;
            _tracks.Clear();
        }

        _headerContextMenuItems.Clear();
        _contentContextMenuItems.Clear();
        PropertyChanged = null;
    }

    #endregion
}
