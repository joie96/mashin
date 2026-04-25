using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
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
    private ObservableRangeCollection<Track> _topTracks = new();
    private ObservableRangeCollection<Track> _recentListens = new();
    private ObservableRangeCollection<Artist> _topArtists = new();
    private ObservableRangeCollection<SimilarArtistSection> _similarArtistSections = new();
    private ObservableRangeCollection<Playlist> _genrePlaylists = new();
    private ObservableRangeCollection<Playlist> _artistPlaylists = new();
    private ObservableRangeCollection<ContextMenuItem> _trackContextMenuItems = new();

    private readonly IReadOnlyList<SlideViewSkeleton> _slideSkeletons = Enumerable.Range(0, 10)
        .Select(_ => new SlideViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<ListViewSkeleton> _listSkeletons = Enumerable.Range(0, 9)
        .Select(_ => new ListViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _rowSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
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

    public ObservableRangeCollection<Track> TopTracks
    {
        get => _topTracks;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Track>();
            if (!EqualityComparer<ObservableRangeCollection<Track>>.Default.Equals(_topTracks, normalizedValue))
            {
                _topTracks = normalizedValue;
                OnPropertyChanged(nameof(TopTracks));
                OnPropertyChanged(nameof(HasTopTracks));
                OnPropertyChanged(nameof(ShowTopTracksRowView));
                OnPropertyChanged(nameof(ShowNoTopTracksMessage));
                OnPropertyChanged(nameof(TopTrackItems));
            }
        }
    }

    public bool HasTopTracks => TopTracks.Count > 0;

    public ObservableRangeCollection<Track> RecentListens
    {
        get => _recentListens;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Track>();
            if (!EqualityComparer<ObservableRangeCollection<Track>>.Default.Equals(_recentListens, normalizedValue))
            {
                _recentListens = normalizedValue;
                OnPropertyChanged(nameof(RecentListens));
                OnPropertyChanged(nameof(HasRecentListens));
                OnPropertyChanged(nameof(ShowRecentListensRowView));
                OnPropertyChanged(nameof(ShowNoRecentListensMessage));
                OnPropertyChanged(nameof(RecentListenItems));
            }
        }
    }

    public bool HasRecentListens => RecentListens.Count > 0;

    public ObservableRangeCollection<Artist> TopArtists
    {
        get => _topArtists;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Artist>();
            if (!EqualityComparer<ObservableRangeCollection<Artist>>.Default.Equals(_topArtists, normalizedValue))
            {
                _topArtists = normalizedValue;
                OnPropertyChanged(nameof(TopArtists));
                OnPropertyChanged(nameof(HasTopArtists));
                OnPropertyChanged(nameof(ShowTopArtistsRowView));
                OnPropertyChanged(nameof(ShowNoTopArtistsMessage));
                OnPropertyChanged(nameof(TopArtistItems));
            }
        }
    }

    public bool HasTopArtists => TopArtists.Count > 0;

    public ObservableRangeCollection<SimilarArtistSection> SimilarArtistSections
    {
        get => _similarArtistSections;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<SimilarArtistSection>();
            if (!EqualityComparer<ObservableRangeCollection<SimilarArtistSection>>.Default.Equals(_similarArtistSections, normalizedValue))
            {
                _similarArtistSections = normalizedValue;
                OnPropertyChanged(nameof(SimilarArtistSections));
                NotifySimilarArtistSectionPropertiesChanged();
            }
        }
    }

    public ObservableRangeCollection<Playlist> GenrePlaylists
    {
        get => _genrePlaylists;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Playlist>();
            if (!EqualityComparer<ObservableRangeCollection<Playlist>>.Default.Equals(_genrePlaylists, normalizedValue))
            {
                _genrePlaylists = normalizedValue;
                OnPropertyChanged(nameof(GenrePlaylists));
                OnPropertyChanged(nameof(HasGenrePlaylists));
                OnPropertyChanged(nameof(ShowGenrePlaylistsListView));
                OnPropertyChanged(nameof(ShowNoGenrePlaylistsMessage));
                OnPropertyChanged(nameof(GenrePlaylistItems));
            }
        }
    }

    public bool HasGenrePlaylists => GenrePlaylists.Count > 0;

    public ObservableRangeCollection<Playlist> ArtistPlaylists
    {
        get => _artistPlaylists;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Playlist>();
            if (!EqualityComparer<ObservableRangeCollection<Playlist>>.Default.Equals(_artistPlaylists, normalizedValue))
            {
                _artistPlaylists = normalizedValue;
                OnPropertyChanged(nameof(ArtistPlaylists));
                OnPropertyChanged(nameof(HasArtistPlaylists));
                OnPropertyChanged(nameof(ShowArtistPlaylistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistPlaylistsMessage));
                OnPropertyChanged(nameof(ArtistPlaylistItems));
            }
        }
    }

    public bool HasArtistPlaylists => ArtistPlaylists.Count > 0;

    public bool ShowRecommendationFolderSlideView => IsLoadingHome || HasRecommendationFolderTracks;

    public bool ShowGenrePlaylistsListView => IsLoadingHome || HasGenrePlaylists;

    public bool ShowNoRecommendationFolderMessage => !IsLoadingHome && !HasRecommendationFolderTracks;

    public bool ShowTopTracksRowView => IsLoadingHome || HasTopTracks;

    public bool ShowNoTopTracksMessage => !IsLoadingHome && !HasTopTracks;

    public bool ShowRecentListensRowView => IsLoadingHome || HasRecentListens;

    public bool ShowNoRecentListensMessage => !IsLoadingHome && !HasRecentListens;

    public bool ShowTopArtistsRowView => IsLoadingHome || HasTopArtists;

    public bool ShowNoTopArtistsMessage => !IsLoadingHome && !HasTopArtists;

    public bool ShowSimilarArtistSectionContainer0 => GetShowSimilarArtistSectionContainer(0);

    public bool ShowSimilarArtistSectionContainer1 => GetShowSimilarArtistSectionContainer(1);

    public bool ShowSimilarArtistSectionContainer2 => GetShowSimilarArtistSectionContainer(2);

    public bool ShowSimilarArtistSectionContainer3 => GetShowSimilarArtistSectionContainer(3);

    public bool ShowSimilarArtistSectionContainer4 => GetShowSimilarArtistSectionContainer(4);

    public bool ShowSimilarArtistSectionRowView0 => GetShowSimilarArtistSectionRowView(0);

    public bool ShowSimilarArtistSectionRowView1 => GetShowSimilarArtistSectionRowView(1);

    public bool ShowSimilarArtistSectionRowView2 => GetShowSimilarArtistSectionRowView(2);

    public bool ShowSimilarArtistSectionRowView3 => GetShowSimilarArtistSectionRowView(3);

    public bool ShowSimilarArtistSectionRowView4 => GetShowSimilarArtistSectionRowView(4);

    public bool ShowNoSimilarArtistSectionMessage0 => GetShowNoSimilarArtistSectionMessage(0);

    public bool ShowNoSimilarArtistSectionMessage1 => GetShowNoSimilarArtistSectionMessage(1);

    public bool ShowNoSimilarArtistSectionMessage2 => GetShowNoSimilarArtistSectionMessage(2);

    public bool ShowNoSimilarArtistSectionMessage3 => GetShowNoSimilarArtistSectionMessage(3);

    public bool ShowNoSimilarArtistSectionMessage4 => GetShowNoSimilarArtistSectionMessage(4);

    public bool ShowNoGenrePlaylistsMessage => !IsLoadingHome && !HasGenrePlaylists;

    public bool ShowArtistPlaylistsRowView => IsLoadingHome || HasArtistPlaylists;

    public bool ShowNoArtistPlaylistsMessage => !IsLoadingHome && !HasArtistPlaylists;

    public IEnumerable<object> RecommendationTrackItems => IsLoadingHome ? _slideSkeletons : _recommendationTracks;

    public IEnumerable<object> TopTrackItems => IsLoadingHome ? _rowSkeletons : _topTracks;

    public IEnumerable<object> RecentListenItems => IsLoadingHome ? _rowSkeletons : _recentListens;

    public IEnumerable<object> TopArtistItems => IsLoadingHome ? _rowSkeletons : _topArtists;

    public string SimilarArtistSectionTitle0 => GetSimilarArtistSectionTitle(0);

    public string SimilarArtistSectionTitle1 => GetSimilarArtistSectionTitle(1);

    public string SimilarArtistSectionTitle2 => GetSimilarArtistSectionTitle(2);

    public string SimilarArtistSectionTitle3 => GetSimilarArtistSectionTitle(3);

    public string SimilarArtistSectionTitle4 => GetSimilarArtistSectionTitle(4);

    public int SimilarArtistSectionCount0 => GetSimilarArtistSectionCount(0);

    public int SimilarArtistSectionCount1 => GetSimilarArtistSectionCount(1);

    public int SimilarArtistSectionCount2 => GetSimilarArtistSectionCount(2);

    public int SimilarArtistSectionCount3 => GetSimilarArtistSectionCount(3);

    public int SimilarArtistSectionCount4 => GetSimilarArtistSectionCount(4);

    public IEnumerable<object> SimilarArtistSectionItems0 => GetSimilarArtistSectionItems(0);

    public IEnumerable<object> SimilarArtistSectionItems1 => GetSimilarArtistSectionItems(1);

    public IEnumerable<object> SimilarArtistSectionItems2 => GetSimilarArtistSectionItems(2);

    public IEnumerable<object> SimilarArtistSectionItems3 => GetSimilarArtistSectionItems(3);

    public IEnumerable<object> SimilarArtistSectionItems4 => GetSimilarArtistSectionItems(4);

    public IEnumerable<object> GenrePlaylistItems => IsLoadingHome ? _listSkeletons : _genrePlaylists;

    public IEnumerable<object> ArtistPlaylistItems => IsLoadingHome ? _rowSkeletons : _artistPlaylists;

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
                OnPropertyChanged(nameof(ShowTopTracksRowView));
                OnPropertyChanged(nameof(ShowNoTopTracksMessage));
                OnPropertyChanged(nameof(TopTrackItems));
                OnPropertyChanged(nameof(ShowRecentListensRowView));
                OnPropertyChanged(nameof(ShowNoRecentListensMessage));
                OnPropertyChanged(nameof(RecentListenItems));
                OnPropertyChanged(nameof(ShowTopArtistsRowView));
                OnPropertyChanged(nameof(ShowNoTopArtistsMessage));
                OnPropertyChanged(nameof(TopArtistItems));
                NotifySimilarArtistSectionPropertiesChanged();
                OnPropertyChanged(nameof(ShowGenrePlaylistsListView));
                OnPropertyChanged(nameof(ShowNoGenrePlaylistsMessage));
                OnPropertyChanged(nameof(GenrePlaylistItems));
                OnPropertyChanged(nameof(ShowArtistPlaylistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistPlaylistsMessage));
                OnPropertyChanged(nameof(ArtistPlaylistItems));
            }
        }
    }

    public IMediaItemActions MediaActions => _mediaActions;

    public ICommand AlbumTappedCommand { get; }

    public ICommand ArtistTappedCommand { get; }

    public ICommand PlaylistTappedCommand { get; }

    public ICommand RecommendationTrackTappedCommand { get; }

    public ICommand ShowTrackContextMenuAtAnchorCommand { get; }

    public ICommand ShowTrackContextMenuAtPositionCommand { get; }

    public ICommand PlayArtistPlaylistsCommand { get; }

    public ICommand ShuffleArtistPlaylistsCommand { get; }

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

        // Navigation commands
        AlbumTappedCommand = new Command<object>(async parameter =>
            await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter =>
            await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

        PlaylistTappedCommand = new Command<object>(async parameter =>
            await _navigationService.NavigateToAsync<PlaylistDetailPage>(parameter));

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

        PlayArtistPlaylistsCommand = new Command(async () => await PlayArtistPlaylistsAsync());

        ShuffleArtistPlaylistsCommand = new Command(async () => await ShuffleArtistPlaylistsAsync());
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
            await LoadTopTracksAsync(lbFolders);
            await LoadRecentListensAsync();
            await LoadTopArtistsAsync(lbFolders);
            await LoadSimilarArtistsSectionsAsync(lbFolders);
            await LoadGenrePlaylistsAsync(lbFolders);
            await LoadArtistPlaylistsAsync(lbFolders);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home sections");
            RecommendationTracks = new ObservableRangeCollection<Track>();
            TopTracks = new ObservableRangeCollection<Track>();
            RecentListens = new ObservableRangeCollection<Track>();
            TopArtists = new ObservableRangeCollection<Artist>();
            SimilarArtistSections = new ObservableRangeCollection<SimilarArtistSection>();
            GenrePlaylists = new ObservableRangeCollection<Playlist>();
            ArtistPlaylists = new ObservableRangeCollection<Playlist>();
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

        // Shuffle recommendations on each load so the slide order varies.
        for (var i = recommendationTracks.Count - 1; i > 0; i--)
        {
            var swapIndex = Random.Shared.Next(i + 1);
            (recommendationTracks[i], recommendationTracks[swapIndex]) = (recommendationTracks[swapIndex], recommendationTracks[i]);
        }

        RecommendationTracks = new ObservableRangeCollection<Track>(recommendationTracks);
        _ = BuildTrackContextMenuAsync();

        _logger.LogInformation(
            "Home recommendation folder loaded: recommendationTracks={RecommendationTracks}",
            recommendationTracks.Count);
    }

    private async Task LoadTopTracksAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        var topTracks = FindRecommendationFolderById(lbFolders, "top_tracks")?.Items?
            .OfType<Track>()
            .Where(track => !string.IsNullOrWhiteSpace(track.ItemId) && !string.IsNullOrWhiteSpace(track.Provider))
            .Select(track =>
            {
                track.DisplayName = track.Name;
                return track;
            })
            .ToList()
            ?? new List<Track>();

        await _musicAssistant.EnrichWithProviderInfoAsync(topTracks);

        TopTracks = new ObservableRangeCollection<Track>(topTracks);

        _logger.LogInformation(
            "Home top tracks loaded: topTracks={TopTracks}",
            topTracks.Count);
    }

    private async Task LoadRecentListensAsync()
    {
        var currentUser = await _musicAssistant.GetCurrentUserAsync();
        var recentItems = await _musicAssistant.GetRecentlyPlayedItemsAsync(
            limit: 50,
            mediaTypes: new[] { MediaType.Track },
            userId: currentUser?.UserId);

        var recentTrackRefs = recentItems
            .OfType<Track>()
            .Where(track => !string.IsNullOrWhiteSpace(track.ItemId) && !string.IsNullOrWhiteSpace(track.Provider))
            .ToList();

        var fullTrackTasks = recentTrackRefs.Select(async trackRef =>
        {
            try
            {
                var fullTrack = await _musicAssistant.GetTrackAsync(trackRef.ItemId, trackRef.Provider);
                return fullTrack ?? trackRef;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to load full recent track details for track: {TrackId}", trackRef.ItemId);
                return trackRef;
            }
        });

        var recentTracks = (await Task.WhenAll(fullTrackTasks))
            .Where(track => track != null)
            .Select(track =>
            {
                track!.DisplayName = track.Name;
                return track;
            })
            .ToList();

        await _musicAssistant.EnrichWithProviderInfoAsync(recentTracks);

        RecentListens = new ObservableRangeCollection<Track>(recentTracks);

        _logger.LogInformation(
            "Home recent listens loaded: recentTracks={RecentTracks}, userId={UserId}",
            recentTracks.Count,
            currentUser?.UserId ?? string.Empty);
    }

    private async Task LoadTopArtistsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        var topArtists = FindRecommendationFolderById(lbFolders, "top_artists")?.Items?
            .OfType<Artist>()
            .Where(artist => !string.IsNullOrWhiteSpace(artist.ItemId) && !string.IsNullOrWhiteSpace(artist.Provider))
            .Select(artist =>
            {
                artist.DisplayName = artist.Name;
                return artist;
            })
            .ToList()
            ?? new List<Artist>();

        await _musicAssistant.EnrichWithProviderInfoAsync(topArtists);

        TopArtists = new ObservableRangeCollection<Artist>(topArtists);

        _logger.LogInformation(
            "Home top artists loaded: topArtists={TopArtists}",
            topArtists.Count);
    }

    private async Task LoadSimilarArtistsSectionsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        var similarArtistSections = new List<SimilarArtistSection>();

        var similarArtistFolders = lbFolders
            .Where(folder =>
                !string.IsNullOrWhiteSpace(folder.ItemId)
                && folder.ItemId.StartsWith("similar_artists_", StringComparison.OrdinalIgnoreCase))
            .ToList();

        foreach (var folder in similarArtistFolders)
        {
            var artists = folder.Items?
                .OfType<Artist>()
                .Where(artist => !string.IsNullOrWhiteSpace(artist.ItemId) && !string.IsNullOrWhiteSpace(artist.Provider))
                .Select(artist =>
                {
                    artist.DisplayName = artist.Name;
                    return artist;
                })
                .ToList()
                ?? new List<Artist>();

            await _musicAssistant.EnrichWithProviderInfoAsync(artists);

            var sectionName = string.IsNullOrWhiteSpace(folder.Name) ? folder.ItemId : folder.Name;
            sectionName = sectionName.Replace("Similar Artists for", "Ähnlich zu", StringComparison.OrdinalIgnoreCase);

            similarArtistSections.Add(new SimilarArtistSection(
                sectionName,
                new ObservableRangeCollection<Artist>(artists)));
        }

        // Shuffle section order so the displayed folders vary on each load.
        for (var i = similarArtistSections.Count - 1; i > 0; i--)
        {
            var swapIndex = Random.Shared.Next(i + 1);
            (similarArtistSections[i], similarArtistSections[swapIndex]) = (similarArtistSections[swapIndex], similarArtistSections[i]);
        }

        SimilarArtistSections = new ObservableRangeCollection<SimilarArtistSection>(similarArtistSections);

        _logger.LogInformation(
            "Home similar artists sections loaded: similarArtistSections={SimilarArtistSections}",
            similarArtistSections.Count);
    }

    private async Task LoadGenrePlaylistsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        var genrePlaylists = FindRecommendationFolderById(lbFolders, "genre_playlists")?.Items?
            .OfType<Playlist>()
            .Where(playlist => !string.IsNullOrWhiteSpace(playlist.ItemId) && !string.IsNullOrWhiteSpace(playlist.Provider))
            .Select(playlist =>
            {
                var playlistName = playlist.Name ?? string.Empty;
                playlist.DisplayName = playlistName.StartsWith("Radio: ", StringComparison.OrdinalIgnoreCase)
                    ? playlistName[7..].TrimStart()
                    : playlistName;
                return playlist;
            })
            .ToList()
            ?? new List<Playlist>();

        await _musicAssistant.EnrichWithProviderInfoAsync(genrePlaylists);

        GenrePlaylists = new ObservableRangeCollection<Playlist>(genrePlaylists);

        _logger.LogInformation(
            "Home genre playlists loaded: genrePlaylists={GenrePlaylists}",
            genrePlaylists.Count);
    }

    private async Task LoadArtistPlaylistsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        var artistPlaylists = FindRecommendationFolderById(lbFolders, "artist_playlists")?.Items?
            .OfType<Playlist>()
            .Where(playlist => !string.IsNullOrWhiteSpace(playlist.ItemId) && !string.IsNullOrWhiteSpace(playlist.Provider))
            .Select(playlist =>
            {
                playlist.DisplayName = NormalizePlaylistDisplayName(playlist.Name);
                return playlist;
            })
            .ToList()
            ?? new List<Playlist>();

        await _musicAssistant.EnrichWithProviderInfoAsync(artistPlaylists);

        ArtistPlaylists = new ObservableRangeCollection<Playlist>(artistPlaylists);

        _logger.LogInformation(
            "Home artist playlists loaded: artistPlaylists={ArtistPlaylists}",
            artistPlaylists.Count);
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

    private static string NormalizePlaylistDisplayName(string? playlistName)
    {
        var normalizedPlaylistName = playlistName ?? string.Empty;
        return normalizedPlaylistName.StartsWith("Radio: ", StringComparison.OrdinalIgnoreCase)
            ? normalizedPlaylistName[7..].TrimStart()
            : normalizedPlaylistName;
    }

    private SimilarArtistSection? GetSimilarArtistSection(int index)
    {
        if (index < 0 || index >= SimilarArtistSections.Count)
        {
            return null;
        }

        return SimilarArtistSections[index];
    }

    private string GetSimilarArtistSectionTitle(int index)
    {
        return GetSimilarArtistSection(index)?.Name ?? string.Empty;
    }

    private int GetSimilarArtistSectionCount(int index)
    {
        if (IsLoadingHome)
        {
            return _rowSkeletons.Count;
        }

        return GetSimilarArtistSection(index)?.Artists.Count ?? 0;
    }

    private bool GetShowSimilarArtistSectionContainer(int index)
    {
        return IsLoadingHome || GetSimilarArtistSection(index) != null;
    }

    private bool GetShowSimilarArtistSectionRowView(int index)
    {
        return IsLoadingHome || (GetSimilarArtistSection(index)?.Artists.Count > 0);
    }

    private bool GetShowNoSimilarArtistSectionMessage(int index)
    {
        var section = GetSimilarArtistSection(index);
        return !IsLoadingHome && section != null && section.Artists.Count == 0;
    }

    private IEnumerable<object> GetSimilarArtistSectionItems(int index)
    {
        if (IsLoadingHome)
        {
            return _rowSkeletons;
        }

        return GetSimilarArtistSection(index)?.Artists ?? Enumerable.Empty<Artist>();
    }

    private void NotifySimilarArtistSectionPropertiesChanged()
    {
        OnPropertyChanged(nameof(ShowSimilarArtistSectionContainer0));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionContainer1));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionContainer2));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionContainer3));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionContainer4));

        OnPropertyChanged(nameof(ShowSimilarArtistSectionRowView0));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionRowView1));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionRowView2));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionRowView3));
        OnPropertyChanged(nameof(ShowSimilarArtistSectionRowView4));

        OnPropertyChanged(nameof(ShowNoSimilarArtistSectionMessage0));
        OnPropertyChanged(nameof(ShowNoSimilarArtistSectionMessage1));
        OnPropertyChanged(nameof(ShowNoSimilarArtistSectionMessage2));
        OnPropertyChanged(nameof(ShowNoSimilarArtistSectionMessage3));
        OnPropertyChanged(nameof(ShowNoSimilarArtistSectionMessage4));

        OnPropertyChanged(nameof(SimilarArtistSectionTitle0));
        OnPropertyChanged(nameof(SimilarArtistSectionTitle1));
        OnPropertyChanged(nameof(SimilarArtistSectionTitle2));
        OnPropertyChanged(nameof(SimilarArtistSectionTitle3));
        OnPropertyChanged(nameof(SimilarArtistSectionTitle4));

        OnPropertyChanged(nameof(SimilarArtistSectionCount0));
        OnPropertyChanged(nameof(SimilarArtistSectionCount1));
        OnPropertyChanged(nameof(SimilarArtistSectionCount2));
        OnPropertyChanged(nameof(SimilarArtistSectionCount3));
        OnPropertyChanged(nameof(SimilarArtistSectionCount4));

        OnPropertyChanged(nameof(SimilarArtistSectionItems0));
        OnPropertyChanged(nameof(SimilarArtistSectionItems1));
        OnPropertyChanged(nameof(SimilarArtistSectionItems2));
        OnPropertyChanged(nameof(SimilarArtistSectionItems3));
        OnPropertyChanged(nameof(SimilarArtistSectionItems4));
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

    private async Task PlayArtistPlaylistsAsync()
    {
        var playlists = ArtistPlaylists.ToList();
        if (playlists.Count == 0)
        {
            return;
        }

        await _mediaActions.PlayMediaAsync(playlists[0]);
    }

    private async Task ShuffleArtistPlaylistsAsync()
    {
        var playlists = ArtistPlaylists.ToList();
        if (playlists.Count == 0)
        {
            return;
        }

        var randomIndex = Random.Shared.Next(playlists.Count);
        await _mediaActions.PlayMediaAsync(playlists[randomIndex]);
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
        _topTracks.Clear();
        _recentListens.Clear();
        _topArtists.Clear();
        _similarArtistSections.Clear();
        _genrePlaylists.Clear();
        _artistPlaylists.Clear();
        _trackContextMenuItems.Clear();

        PropertyChanged = null;
    }

    #endregion

    #region INotifyPropertyChanged

    public sealed class SimilarArtistSection
    {
        public SimilarArtistSection(string name, ObservableRangeCollection<Artist> artists)
        {
            Name = name;
            Artists = artists;
        }

        public string Name { get; }

        public ObservableRangeCollection<Artist> Artists { get; }
    }

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
