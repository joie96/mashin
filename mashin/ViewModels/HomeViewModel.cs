using mashin.Collections;
using mashin.Converters;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Text.Json.Serialization;
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

    private ObservableRangeCollection<Artist> _weeklyTopArtists = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();

    private readonly IReadOnlyList<RowViewSkeleton> _artistSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();

    private static readonly JsonSerializerOptions RecommendationItemJsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        Converters =
        {
            new JsonStringEnumConverter(),
            new FlexibleIntConverter()
        }
    };

    private bool _isLoadingWeeklyTopArtists;
    private bool _disposed;

    #endregion

    #region Properties

    public ObservableRangeCollection<Artist> WeeklyTopArtists
    {
        get => _weeklyTopArtists;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Artist>();
            if (_weeklyTopArtists != null)
            {
                _weeklyTopArtists.CollectionChanged -= OnWeeklyTopArtistsCollectionChanged;
            }

            if (!EqualityComparer<ObservableRangeCollection<Artist>>.Default.Equals(_weeklyTopArtists, normalizedValue))
            {
                _weeklyTopArtists = normalizedValue;
                OnPropertyChanged(nameof(WeeklyTopArtists));

                if (_weeklyTopArtists != null)
                {
                    _weeklyTopArtists.CollectionChanged += OnWeeklyTopArtistsCollectionChanged;
                }

                OnPropertyChanged(nameof(HasWeeklyTopArtists));
                OnPropertyChanged(nameof(ShowArtistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistsMessage));
                OnPropertyChanged(nameof(ArtistItems));
            }
        }
    }

    public bool HasWeeklyTopArtists => WeeklyTopArtists.Count > 0;
    public bool ShowArtistsRowView => IsLoadingWeeklyTopArtists || HasWeeklyTopArtists;
    public bool ShowNoArtistsMessage => !IsLoadingWeeklyTopArtists && !HasWeeklyTopArtists;
    public IEnumerable<object> ArtistItems => IsLoadingWeeklyTopArtists ? _artistSkeletons : _weeklyTopArtists;

    public bool IsLoadingWeeklyTopArtists
    {
        get => _isLoadingWeeklyTopArtists;
        private set
        {
            if (SetProperty(ref _isLoadingWeeklyTopArtists, value))
            {
                OnPropertyChanged(nameof(ShowArtistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistsMessage));
                OnPropertyChanged(nameof(ArtistItems));
            }
        }
    }

    public IMediaItemActions MediaActions => _mediaActions;

    public ICommand ArtistTappedCommand { get; }
    public ICommand ShowArtistContextMenuAtAnchorCommand { get; }
    public ICommand ShowArtistContextMenuAtPositionCommand { get; }

    #endregion

    #region Collection Changed Handlers

    private void OnWeeklyTopArtistsCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasWeeklyTopArtists));
        OnPropertyChanged(nameof(ShowArtistsRowView));
        OnPropertyChanged(nameof(ShowNoArtistsMessage));
        OnPropertyChanged(nameof(ArtistItems));
    }

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

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

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
        _logger.LogInformation("Loading home recommendations");
        _ = LoadWeeklyTopArtistsAsync();
        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from home");
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    private async Task LoadWeeklyTopArtistsAsync()
    {
        IsLoadingWeeklyTopArtists = true;

        try
        {
            var recommendations = await _musicAssistant.GetRecommendationsAsync();
            var weeklyArtistsFolder = recommendations.FirstOrDefault(folder =>
                !string.IsNullOrWhiteSpace(folder.Provider)
                && folder.Provider.StartsWith("listenbrainz_recommendations" + "--", StringComparison.OrdinalIgnoreCase)
                &&
                string.Equals(folder.ItemId, "sitewide_artists_week", StringComparison.OrdinalIgnoreCase));

            var artists = ParseArtistsFromRecommendationFolder(weeklyArtistsFolder);
            await _musicAssistant.EnrichWithProviderInfoAsync(artists);

            WeeklyTopArtists = new ObservableRangeCollection<Artist>(artists);
            _ = BuildArtistContextMenuAsync();

            _logger.LogInformation("Weekly top artists loaded: {Count}", artists.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load weekly top artists from recommendations");
            WeeklyTopArtists = new ObservableRangeCollection<Artist>();
        }
        finally
        {
            IsLoadingWeeklyTopArtists = false;
        }
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
                    await _mediaActions.PlayMediaAsync(WeeklyTopArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaNextAsync(WeeklyTopArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaLastAsync(WeeklyTopArtists.Where(a => a.IsSelected)))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await _mediaActions.AddToFavoritesAsync(WeeklyTopArtists.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await _mediaActions.RemoveFromFavoritesAsync(WeeklyTopArtists.Where(a => a.IsSelected)))
            }
        };

        return Task.CompletedTask;
    }

    #endregion

    #region Helpers

    private static List<Artist> ParseArtistsFromRecommendationFolder(RecommendationFolder? folder)
    {
        if (folder?.Items == null || folder.Items.Count == 0)
        {
            return new List<Artist>();
        }

        var artists = new List<Artist>();

        foreach (var item in folder.Items)
        {
            if (item.ValueKind != JsonValueKind.Object)
            {
                continue;
            }

            if (!item.TryGetProperty("media_type", out var mediaTypeProperty)
                || !string.Equals(mediaTypeProperty.GetString(), "artist", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var artist = JsonSerializer.Deserialize<Artist>(item.GetRawText(), RecommendationItemJsonOptions);
            if (artist == null || string.IsNullOrWhiteSpace(artist.ItemId) || string.IsNullOrWhiteSpace(artist.Provider))
            {
                continue;
            }

            artist.DisplayName = artist.Name;
            artists.Add(artist);
        }

        return artists;
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

        _weeklyTopArtists.CollectionChanged -= OnWeeklyTopArtistsCollectionChanged;

        _artistContextMenuItems.Clear();
        _weeklyTopArtists.Clear();

        PropertyChanged = null;
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        if (propertyName == nameof(IsLoadingWeeklyTopArtists) && !IsLoadingWeeklyTopArtists)
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
