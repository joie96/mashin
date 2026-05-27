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
    private readonly IUserDataService _userDataService;
    private readonly IOverlayService _overlayService;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<PlaylistDetailViewModel> _logger;
    private readonly Random _shuffleRandom = new();

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

    public IMediaItemActions MediaActions { get; }
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
        IPlayerService playerService,
        IUserDataService userDataService,
        IOverlayService overlayService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<PlaylistDetailViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _overlayService = overlayService;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;

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
            if (_contentContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_contentContextMenuItems, anchor);
            }
        });

        ShowContentContextMenuAtPositionCommand = new Command<Point>(async (position) =>
        {
            if (_contentContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_contentContextMenuItems, position);
            }
        });

        PlayPlaylistCommand = new Command(async () =>
        {
            if (Playlist != null)
            {
                await MediaActions.PlayMediaAsync(Playlist);
            }
        });

        ShufflePlaylistCommand = new Command(async () =>
        {
            var shuffledTracks = Tracks.ToList();
            if (shuffledTracks.Count == 0)
            {
                return;
            }

            for (var i = shuffledTracks.Count - 1; i > 0; i--)
            {
                var j = _shuffleRandom.Next(i + 1);
                (shuffledTracks[i], shuffledTracks[j]) = (shuffledTracks[j], shuffledTracks[i]);
            }

            await MediaActions.PlayMediaAsync(shuffledTracks[0]);

            var remainingTracks = shuffledTracks.Skip(1).ToList();
            if (remainingTracks.Count > 0)
            {
                await MediaActions.PlayMediaNextAsync(remainingTracks);
            }
        });

        TogglePlaylistFavoriteCommand = new Command(async () =>
        {
            if (Playlist == null)
            {
                return;
            }

            if (Playlist.Favorite)
            {
                await MediaActions.RemoveFromFavoritesAsync(Playlist);
            }
            else
            {
                await MediaActions.AddToFavoritesAsync(Playlist);
            }

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
            _logger.LogInformation("Navigated to playlist target: {ItemId} ({Provider})", item.ItemId, item.Provider);
            
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
            await LoadPlaylistMetadataAsync(playlistId, providerInstanceOrDomain);

            await LoadPlaylistTracksAsync(playlistId, providerInstanceOrDomain);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load playlist: {PlaylistId}", playlistId);
        }
    }

    private async Task LoadPlaylistMetadataAsync(string playlistId, string providerInstanceOrDomain)
    {
        try
        {
            var playlist = await _musicAssistant.GetPlaylistAsync(playlistId, providerInstanceOrDomain);
            if (playlist != null)
            {
                playlist.Favorite = await _userDataService.IsFavoriteAsync(playlist);
            }

            if (playlist != null)
            {
                var prefix = GetUserPlaylistPrefix();
                playlist.DisplayName = playlist.Name;

                if (!string.IsNullOrWhiteSpace(prefix)
                    && !string.IsNullOrWhiteSpace(playlist.Name)
                    && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                {
                    playlist.DisplayName = playlist.Name[prefix.Length..];
                }
            }

            Playlist = playlist;

            if (Playlist == null)
            {
                _logger.LogWarning("Playlist not found: {PlaylistId}", playlistId);
                return;
            }

            _ = BuildHeaderContextMenuAsync();
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
            var tracks = await _musicAssistant.GetPlaylistTracksAsync(
                playlistId,
                providerInstanceOrDomain,
                forceRefresh: true);

            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = i + 1;
                tracks[i].Favorite = await _userDataService.IsFavoriteAsync(tracks[i]);
            }

            // Load tracks progressively
            Tracks = new ObservableRangeCollection<Track>(tracks.ToList());
            IsLoadingTracks = false;

            _ = BuildContentContextMenuAsync();

            if (Playlist != null)
            {
                _logger.LogInformation("Loaded playlist '{Name}' with {Count} tracks",
                    Playlist.Name, Tracks.Count);
            }
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

        await _userDataService.GetPreferencesAsync();
        var prefix = GetUserPlaylistPrefix();
        if (!string.IsNullOrWhiteSpace(prefix)
            && !updatedName.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            updatedName = string.Concat(prefix, updatedName);
        }

        var originalName = playlist.Name;
        var originalDisplayName = playlist.DisplayName;

        var renamed = false;
        try
        {
            playlist.Name = updatedName;
            playlist.DisplayName = !string.IsNullOrWhiteSpace(prefix)
                && updatedName.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
                ? updatedName[prefix.Length..]
                : updatedName;

            await MediaActions.UpdatePlaylistAsync(playlist);
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
            await MediaActions.RemovePlaylistAsync(playlist);
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
                Command = new Command(async () => await MediaActions.PlayMediaNextAsync(Playlist))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () => await MediaActions.PlayMediaLastAsync(Playlist))
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
                    await MediaActions.RemoveFromFavoritesAsync(Playlist);
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
                    await MediaActions.AddToFavoritesAsync(Playlist);
                    OnPropertyChanged(nameof(IsPlaylistFavorite));
                    _ = BuildHeaderContextMenuAsync();
                })
            });
        }

        menu.Add(new ContextMenuItem { IsSeparator = true });

        menu.Add(new ContextMenuItem
        {
            Text = "Wiedergabeliste bearbeiten",
            Icon = FluentIcons.Edit16,
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
        var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(orderBy: "sort_name");
        ApplyPlaylistDisplayNames(playlists);

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                {
                    var selectedTracks = Tracks.Where(t => t.IsSelected).ToList();
                    if (selectedTracks.Count == 0)
                    {
                        return;
                    }

                    await MediaActions.PlayMediaAsync(selectedTracks[0]);

                    var remainingTracks = selectedTracks.Skip(1).ToList();
                    if (remainingTracks.Count > 0)
                    {
                        await MediaActions.PlayMediaNextAsync(remainingTracks);
                    }
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () => 
                    await MediaActions.PlayMediaNextAsync(Tracks.Where(t => t.IsSelected)))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () => 
                    await MediaActions.PlayMediaLastAsync(Tracks.Where(t => t.IsSelected)))
            },
            new()
            {
                Text = "Artist öffnen",
                Command = new Command(async () =>
                {
                    var selectedArtist = Tracks
                        .Where(track => track.IsSelected)
                        .SelectMany(track => track.Artists ?? Enumerable.Empty<Artist>())
                        .FirstOrDefault();

                    if (selectedArtist == null)
                    {
                        return;
                    }

                    await _navigationService.NavigateToAsync<ArtistDetailPage>(selectedArtist);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufügen",
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
            new()
            {
                Text = "Aus Wiedergabeliste entfernen",
                Icon = FluentIcons.Subtract12,
                Command = new Command(async () =>
                {
                    if (Playlist != null)
                    {
                        await MediaActions.RemoveFromPlaylistAsync(
                            Tracks.Where(t => t.IsSelected),
                            Playlist);
            
                        // Playlist neu laden, um entfernte Tracks zu aktualisieren
                        await LoadPlaylistAsync(Playlist.ItemId, Playlist.Provider);
                    }
                }),
                IsEnabled = true
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

        _contentContextMenuItems = menu;
    }

    #endregion

    #region Helper Methods
    private string? GetUserPlaylistPrefix()
    {
        var username = _userDataService.CurrentUser?.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
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
        
        _logger.LogInformation("Disposing PlaylistDetailViewModel for playlist: {PlaylistName}", PlaylistName);
        
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