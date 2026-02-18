using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
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
    private readonly IPlaylistStoreService _playlistStore;
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
    private bool _isLoadingTracks;
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
                OnPropertyChanged(nameof(ImageUrl));
                OnPropertyChanged(nameof(IsPlaylistFavorite));
            }
        }
    }

    public string PlaylistName => Playlist?.DisplayName ?? "Unbekannte Playlist";

    public string? ImageUrl => Playlist?.ImageUrl;

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

    public bool HasTracks => Tracks.Count > 0;

    public bool ShowTrackTable => IsLoadingTracks || HasTracks;

    public bool IsPlaylistFavorite => Playlist?.Favorite ?? false;

    public IEnumerable<object> TrackItems => IsLoadingTracks ? _trackSkeletons : _tracks;

    public IMediaItemActions MediaActions { get; }
    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand ShowHeaderContextMenuAtAnchorCommand { get; }
    public ICommand ShowHeaderContextMenuAtPositionCommand { get; }
    public ICommand ShowContentContextMenuAtAnchorCommand { get; }
    public ICommand ShowContentContextMenuAtPositionCommand { get; }
    public ICommand PlayPlaylistCommand { get; }
    public ICommand TogglePlaylistFavoriteCommand { get; }

    #endregion

    #region Collection Changed Handlers

    private void OnTracksCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasTracks));
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
        IPlaylistStoreService playlistStore,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<PlaylistDetailViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _overlayService = overlayService;
        _playlistStore = playlistStore;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

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
            await BuildHeaderContextMenuAsync();
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
        IsLoadingTracks = true;
        try
        {
            // Fetch playlist metadata
            var playlist = await _musicAssistant.GetPlaylistAsync(playlistId, providerInstanceOrDomain);
            playlist?.Favorite = _userDataService.IsFavorite(playlist);

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

            // Fetch Tracks
            var tracks = await _musicAssistant.GetPlaylistTracksAsync(
                playlistId,
                providerInstanceOrDomain,
                forceRefresh: true);

            // Populate track indices and favorite status
            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = i + 1;
                tracks[i].Favorite = _userDataService.IsFavorite(tracks[i]);
            }

            Tracks = new ObservableRangeCollection<Track>();
            _navigationService.IsNavigating = false;

            _ = BuildContentContextMenuAsync();

            // Render in chunks of 10
            foreach (var batch in tracks.Chunk(10))
                {
                    Tracks.AddRange(batch);
                    await Task.Yield();
                }

            _logger.LogInformation("Loaded playlist '{Name}' with {Count} tracks",
                Playlist.Name, Tracks.Count);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load playlist: {PlaylistId}", playlistId);
        }
        finally
        {
            IsLoadingTracks = false;
            _navigationService.IsNavigating = false;
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

        var renamed = await _playlistStore.RenameAsync(playlist, updatedName);
        if (renamed)
        {
            OnPropertyChanged(nameof(PlaylistName));
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

        var removed = await _playlistStore.RemoveAsync(playlist);
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
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () => await MediaActions.PlayMediaAsync(Playlist))
            },
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
                    await BuildHeaderContextMenuAsync();
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
                    await BuildHeaderContextMenuAsync();
                })
            });
        }

        menu.Add(new ContextMenuItem { IsSeparator = true });

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

    private Task BuildContentContextMenuAsync()
    {
        var playlists = _playlistStore.Playlists;

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () => 
                    await MediaActions.PlayMediaAsync(Tracks.Where(t => t.IsSelected)))
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
        return Task.CompletedTask;
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