using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;

namespace mashin.ViewModels;

public sealed class HomeViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IMediaItemActions _mediaActions;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<HomeViewModel> _logger;

    private ObservableRangeCollection<Track> _recommendationTracks = new();
    private ObservableRangeCollection<ContextMenuItem> _trackContextMenuItems = new();

    private readonly IReadOnlyList<SlideViewSkeleton> _slideSkeletons = Enumerable.Range(0, 10)
        .Select(_ => new SlideViewSkeleton())
        .ToList();

    private bool _isLoadingHome;
    private bool _disposed;

    #endregion

    #region Properties

    public ObservableRangeCollection<Track> RecommendationTracks
    {
        get => _recommendationTracks;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Track>();
            if (!EqualityComparer<ObservableRangeCollection<Track>>.Default.Equals(_recommendationTracks, normalizedValue))
            {
                _recommendationTracks = normalizedValue;
                OnPropertyChanged(nameof(RecommendationTracks));
                OnPropertyChanged(nameof(HasRecommendationFolderTracks));
                OnPropertyChanged(nameof(ShowRecommendationFolderSlideView));
                OnPropertyChanged(nameof(ShowNoRecommendationFolderMessage));
                OnPropertyChanged(nameof(RecommendationTrackItems));
            }
        }
    }

    public bool HasRecommendationFolderTracks => RecommendationTracks.Count > 0;

    public bool ShowRecommendationFolderSlideView => IsLoadingHome || HasRecommendationFolderTracks;

    public bool ShowNoRecommendationFolderMessage => !IsLoadingHome && !HasRecommendationFolderTracks;

    public IEnumerable<object> RecommendationTrackItems => IsLoadingHome ? _slideSkeletons : _recommendationTracks;

    public bool IsLoadingHome
    {
        get => _isLoadingHome;
        private set
        {
            if (SetProperty(ref _isLoadingHome, value))
            {
                OnPropertyChanged(nameof(ShowRecommendationFolderSlideView));
                OnPropertyChanged(nameof(ShowNoRecommendationFolderMessage));
                OnPropertyChanged(nameof(RecommendationTrackItems));
            }
        }
    }

    public IMediaItemActions MediaActions => _mediaActions;

    public ICommand RecommendationTrackTappedCommand { get; }

    public ICommand ShowTrackContextMenuAtAnchorCommand { get; }

    public ICommand ShowTrackContextMenuAtPositionCommand { get; }

    #endregion

    #region Construction

    public HomeViewModel(
        MusicAssistantService musicAssistant,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<HomeViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _mediaActions = mediaActions;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        RecommendationTrackTappedCommand = new Command<object>(async parameter =>
        {
            if (parameter is not Track track)
            {
                return;
            }

            await _mediaActions.PlayMediaAsync(track);
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
    }

    #endregion

    #region INavigationAware

    public Task OnNavigatedToAsync(object? parameter)
    {
        _logger.LogInformation("Loading ListenBrainz recommendation folder for home");
        _ = LoadHomeAsync();
        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from home");
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    private async Task LoadHomeAsync()
    {
        IsLoadingHome = true;

        try
        {
            // Get all listenbrainz recommendation folders
            var recommendations = await _musicAssistant.GetRecommendationsAsync();
            var lbFolders = recommendations
                .Where(IsListenBrainzFolder)
                .ToList();

            await LoadRecommendationTracksAsync(lbFolders);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home sections");
            RecommendationTracks = new ObservableRangeCollection<Track>();
        }
        finally
        {
            IsLoadingHome = false;
        }
    }

    private async Task LoadRecommendationTracksAsync(IEnumerable<RecommendationFolder> lbFolders)
    {

        var recommendationTracks = FindRecommendationFolderById(lbFolders, "recommendations")?.Items?
            .OfType<Track>()
            .Where(track => !string.IsNullOrWhiteSpace(track.ItemId) && !string.IsNullOrWhiteSpace(track.Provider))
            .Select(track =>
            {
                track.DisplayName = track.Name;
                return track;
            })
            .ToList()
            ?? new List<Track>();

        await _musicAssistant.EnrichWithProviderInfoAsync(recommendationTracks);

        RecommendationTracks = new ObservableRangeCollection<Track>(recommendationTracks);
        _ = BuildTrackContextMenuAsync();

        _logger.LogInformation(
            "Home recommendation folder loaded: recommendationTracks={RecommendationTracks}",
            recommendationTracks.Count);
    }

    #endregion

    #region Context Menu

    private Task BuildTrackContextMenuAsync()
    {
        _trackContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Command = new Command(async () => await PlaySelectedRecommendationTracksAsync())
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaNextAsync(GetSelectedRecommendationTracks()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaLastAsync(GetSelectedRecommendationTracks()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Command = new Command(async () =>
                    await _mediaActions.AddToFavoritesAsync(GetSelectedRecommendationTracks()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Command = new Command(async () =>
                    await _mediaActions.RemoveFromFavoritesAsync(GetSelectedRecommendationTracks()))
            }
        };

        return Task.CompletedTask;
    }

    #endregion

    #region Helpers

    private static bool IsListenBrainzFolder(RecommendationFolder folder)
    {
        return folder != null
            && !string.IsNullOrWhiteSpace(folder.Provider)
            && folder.Provider.StartsWith("listenbrainz_recommendations--", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(folder.ItemId);
    }

    private static RecommendationFolder? FindRecommendationFolderById(IEnumerable<RecommendationFolder> folders, string itemId)
    {
        return folders.FirstOrDefault(folder =>
            string.Equals(folder.ItemId, itemId, StringComparison.OrdinalIgnoreCase));
    }

    private IEnumerable<Track> GetSelectedRecommendationTracks()
    {
        return RecommendationTracks.Where(track => track.IsSelected).ToList();
    }

    private async Task PlaySelectedRecommendationTracksAsync()
    {
        var selectedTracks = GetSelectedRecommendationTracks().ToList();
        if (selectedTracks.Count == 0)
        {
            return;
        }

        await _mediaActions.PlayMediaAsync(selectedTracks[0]);

        if (selectedTracks.Count > 1)
        {
            await _mediaActions.PlayMediaNextAsync(selectedTracks.Skip(1).ToList());
        }
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

        _recommendationTracks.Clear();
        _trackContextMenuItems.Clear();

        PropertyChanged = null;
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        if (propertyName == nameof(IsLoadingHome) && !IsLoadingHome)
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
