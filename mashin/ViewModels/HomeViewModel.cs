using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using System.Collections;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;

namespace mashin.ViewModels;

/// <summary>
/// Home page view model that loads top artists from ListenBrainz and maps them
/// directly to local Artist view models, including cover art from Cover Art Archive.
/// </summary>
public sealed class HomeViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    // Placeholder values: replace with your own ListenBrainz account data.
    private const int ArtistCoverSize = 500;

    private readonly ListenBrainzService _listenBrainz;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<HomeViewModel> _logger;

    private readonly IReadOnlyList<RowViewSkeleton> _artistSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();

    private ObservableRangeCollection<Artist> _topArtists = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();
    private bool _isLoadingArtists;
    private bool _disposed;

    #endregion

    #region Properties

    public ObservableRangeCollection<Artist> TopArtists
    {
        get => _topArtists;
        private set
        {
            var newValue = value ?? new ObservableRangeCollection<Artist>();
            _topArtists.CollectionChanged -= OnTopArtistsCollectionChanged;

            if (SetProperty(ref _topArtists, newValue))
            {
                _topArtists.CollectionChanged += OnTopArtistsCollectionChanged;

                OnPropertyChanged(nameof(HasTopArtists));
                OnPropertyChanged(nameof(ShowArtistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistsMessage));
                OnPropertyChanged(nameof(TopArtistItems));
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
                OnPropertyChanged(nameof(ShowArtistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistsMessage));
                OnPropertyChanged(nameof(TopArtistItems));
            }
        }
    }

    public bool HasTopArtists => TopArtists.Count > 0;

    public bool ShowArtistsRowView => IsLoadingArtists || HasTopArtists;

    public bool ShowNoArtistsMessage => !IsLoadingArtists && !HasTopArtists;

    public IEnumerable<object> TopArtistItems => IsLoadingArtists ? _artistSkeletons : TopArtists;

    public IMediaItemActions MediaActions { get; }

    public ICommand ArtistTappedCommand { get; }
    public ICommand ShowArtistContextMenuAtAnchorCommand { get; }
    public ICommand ShowArtistContextMenuAtPositionCommand { get; }

    #endregion

    #region Construction

    public HomeViewModel(
        ListenBrainzService listenBrainz,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<HomeViewModel> logger)
    {
        _listenBrainz = listenBrainz;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;

        ArtistTappedCommand = new Command<object>(async parameter =>
            await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

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
        _logger.LogInformation("Loading Home data (ListenBrainz top artists)");
        _ = LoadTopArtistsAsync();
        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from home");
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    private async Task LoadTopArtistsAsync()
    {
        IsLoadingArtists = true;

        try
        {
            var topArtistsPayload = await _listenBrainz.GetSitewideTopArtistsAsync(
                count: 25,
                offset: 0,
                range: LbStatRange.ThisMonth);

            var artists = new List<Artist>();
            foreach (var lbArtist in topArtistsPayload?.Artists ?? [])
            {
                var artist = await BuildArtistFromListenBrainzAsync(lbArtist);
                if (artist != null)
                {
                    artists.Add(artist);
                }
            }

            TopArtists = new ObservableRangeCollection<Artist>(artists);
            
            await BuildArtistContextMenuAsync();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load ListenBrainz top artists for home page");
            TopArtists = new ObservableRangeCollection<Artist>();
        }
        finally
        {
            IsLoadingArtists = false;
        }
    }

    private async Task<Artist?> BuildArtistFromListenBrainzAsync(LbArtistStat lbArtist)
    {
        if (string.IsNullOrWhiteSpace(lbArtist.ArtistName))
        {
            return null;
        }

        string? coverUrl = null;
        if (!string.IsNullOrWhiteSpace(lbArtist.ArtistMbid))
        {
            coverUrl = ListenBrainzService.CoverArtUrl(lbArtist.ArtistMbid, 250);
        }

        return new Artist
        {
            Name = lbArtist.ArtistName,
            DisplayName = $"{lbArtist.ArtistName} ({lbArtist.ListenCount})",
            ItemId = lbArtist.ArtistMbid ?? lbArtist.ArtistName,
            Provider = "listenbrainz",
            Uri = !string.IsNullOrWhiteSpace(lbArtist.ArtistMbid)
                ? $"listenbrainz://artist/{lbArtist.ArtistMbid}"
                : $"listenbrainz://artist/{Uri.EscapeDataString(lbArtist.ArtistName)}",
            Metadata = string.IsNullOrWhiteSpace(coverUrl)
                ? null
                : new MediaItemMetadata
                {
                    Images =
                    [
                        new MediaItemImage
                        {
                            Type = "thumb",
                            Path = coverUrl,
                            Provider = "builtin",
                            RemotelyAccessible = true
                        }
                    ]
                }
        };
    }

    #endregion

    #region Context Menu

    private Task BuildArtistContextMenuAsync()
    {
        _artistContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaAsync(TopArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaNextAsync(TopArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await MediaActions.PlayMediaLastAsync(TopArtists.Where(a => a.IsSelected)))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(TopArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(TopArtists.Where(a => a.IsSelected)))
            }
        };

        return Task.CompletedTask;
    }

    #endregion

    #region Collection Changed

    private void OnTopArtistsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasTopArtists));
        OnPropertyChanged(nameof(ShowArtistsRowView));
        OnPropertyChanged(nameof(ShowNoArtistsMessage));
        OnPropertyChanged(nameof(TopArtistItems));
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

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

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    #endregion

    #region IDisposable

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;

        if (_topArtists != null)
        {
            _topArtists.CollectionChanged -= OnTopArtistsCollectionChanged;
        }
    }

    #endregion
}
