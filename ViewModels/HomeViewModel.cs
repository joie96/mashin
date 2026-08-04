using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows.Input;

namespace mashin.ViewModels;

public sealed class HomeViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly UserDataService _userDataService;
    private readonly PlaybackService _playbackService;
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
    private ObservableRangeCollection<ContextMenuItem> _playlistContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();

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
    private bool _isLoadingRecommendationTracks;
    private bool _isLoadingTopTracks;
    private bool _isLoadingRecentListens;
    private bool _isLoadingTopArtists;
    private bool _isLoadingSimilarArtistSections;
    private bool _isLoadingGenrePlaylists;
    private bool _isLoadingArtistPlaylists;
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

    public bool ShowRecommendationFolderSlideView => IsLoadingRecommendationTracks || HasRecommendationFolderTracks;

    public bool ShowGenrePlaylistsListView => IsLoadingGenrePlaylists || HasGenrePlaylists;

    public bool ShowNoRecommendationFolderMessage => !IsLoadingRecommendationTracks && !HasRecommendationFolderTracks;

    public bool ShowTopTracksRowView => IsLoadingTopTracks || HasTopTracks;

    public bool ShowNoTopTracksMessage => !IsLoadingTopTracks && !HasTopTracks;

    public bool ShowRecentListensRowView => IsLoadingRecentListens || HasRecentListens;

    public bool ShowNoRecentListensMessage => !IsLoadingRecentListens && !HasRecentListens;

    public bool ShowTopArtistsRowView => IsLoadingTopArtists || HasTopArtists;

    public bool ShowNoTopArtistsMessage => !IsLoadingTopArtists && !HasTopArtists;

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

    public bool ShowNoGenrePlaylistsMessage => !IsLoadingGenrePlaylists && !HasGenrePlaylists;

    public bool ShowArtistPlaylistsRowView => IsLoadingArtistPlaylists || HasArtistPlaylists;

    public bool ShowNoArtistPlaylistsMessage => !IsLoadingArtistPlaylists && !HasArtistPlaylists;

    public IEnumerable<object> RecommendationTrackItems => IsLoadingRecommendationTracks ? _slideSkeletons : _recommendationTracks;

    public IEnumerable<object> TopTrackItems => IsLoadingTopTracks ? _rowSkeletons : _topTracks;

    public IEnumerable<object> RecentListenItems => IsLoadingRecentListens ? _rowSkeletons : _recentListens;

    public IEnumerable<object> TopArtistItems => IsLoadingTopArtists ? _rowSkeletons : _topArtists;

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

    public IEnumerable<object> GenrePlaylistItems => IsLoadingGenrePlaylists ? _listSkeletons : _genrePlaylists;

    public IEnumerable<object> ArtistPlaylistItems => IsLoadingArtistPlaylists ? _rowSkeletons : _artistPlaylists;

    public bool IsLoadingHome
    {
        get => _isLoadingHome;
        private set
        {
            if (SetProperty(ref _isLoadingHome, value))
            {
            }
        }
    }

    public bool IsLoadingRecommendationTracks
    {
        get => _isLoadingRecommendationTracks;
        private set
        {
            if (SetProperty(ref _isLoadingRecommendationTracks, value))
            {
                OnPropertyChanged(nameof(ShowRecommendationFolderSlideView));
                OnPropertyChanged(nameof(ShowNoRecommendationFolderMessage));
                OnPropertyChanged(nameof(RecommendationTrackItems));
            }
        }
    }

    public bool IsLoadingTopTracks
    {
        get => _isLoadingTopTracks;
        private set
        {
            if (SetProperty(ref _isLoadingTopTracks, value))
            {
                OnPropertyChanged(nameof(ShowTopTracksRowView));
                OnPropertyChanged(nameof(ShowNoTopTracksMessage));
                OnPropertyChanged(nameof(TopTrackItems));
            }
        }
    }

    public bool IsLoadingRecentListens
    {
        get => _isLoadingRecentListens;
        private set
        {
            if (SetProperty(ref _isLoadingRecentListens, value))
            {
                OnPropertyChanged(nameof(ShowRecentListensRowView));
                OnPropertyChanged(nameof(ShowNoRecentListensMessage));
                OnPropertyChanged(nameof(RecentListenItems));
            }
        }
    }

    public bool IsLoadingTopArtists
    {
        get => _isLoadingTopArtists;
        private set
        {
            if (SetProperty(ref _isLoadingTopArtists, value))
            {
                OnPropertyChanged(nameof(ShowTopArtistsRowView));
                OnPropertyChanged(nameof(ShowNoTopArtistsMessage));
                OnPropertyChanged(nameof(TopArtistItems));
            }
        }
    }

    public bool IsLoadingSimilarArtistSections
    {
        get => _isLoadingSimilarArtistSections;
        private set
        {
            if (SetProperty(ref _isLoadingSimilarArtistSections, value))
            {
                NotifySimilarArtistSectionPropertiesChanged();
            }
        }
    }

    public bool IsLoadingGenrePlaylists
    {
        get => _isLoadingGenrePlaylists;
        private set
        {
            if (SetProperty(ref _isLoadingGenrePlaylists, value))
            {
                OnPropertyChanged(nameof(ShowGenrePlaylistsListView));
                OnPropertyChanged(nameof(ShowNoGenrePlaylistsMessage));
                OnPropertyChanged(nameof(GenrePlaylistItems));
            }
        }
    }

    public bool IsLoadingArtistPlaylists
    {
        get => _isLoadingArtistPlaylists;
        private set
        {
            if (SetProperty(ref _isLoadingArtistPlaylists, value))
            {
                OnPropertyChanged(nameof(ShowArtistPlaylistsRowView));
                OnPropertyChanged(nameof(ShowNoArtistPlaylistsMessage));
                OnPropertyChanged(nameof(ArtistPlaylistItems));
            }
        }
    }

    public UserDataService UserDataService => _userDataService;

    public ICommand AlbumTappedCommand { get; }

    public ICommand ArtistTappedCommand { get; }

    public ICommand PlaylistTappedCommand { get; }

    public ICommand RecommendationTrackTappedCommand { get; }

    public ICommand ShowTrackContextMenuAtAnchorCommand { get; }

    public ICommand ShowTrackContextMenuAtPositionCommand { get; }

    public ICommand ShowPlaylistContextMenuAtAnchorCommand { get; }

    public ICommand ShowPlaylistContextMenuAtPositionCommand { get; }

    public ICommand ShowArtistContextMenuAtAnchorCommand { get; }

    public ICommand ShowArtistContextMenuAtPositionCommand { get; }

    public ICommand PlayArtistPlaylistsCommand { get; }

    public ICommand ShuffleArtistPlaylistsCommand { get; }

    public ICommand PlayRecommendationTracksCommand { get; }

    public ICommand ShuffleRecommendationTracksCommand { get; }

    public ICommand PlayGenrePlaylistsCommand { get; }

    public ICommand ShuffleGenrePlaylistsCommand { get; }

    public ICommand PlayTopTracksCommand { get; }

    public ICommand ShuffleTopTracksCommand { get; }

    public ICommand PlayTopArtistsCommand { get; }

    public ICommand ShuffleTopArtistsCommand { get; }

    public ICommand PlayRecentListensCommand { get; }

    public ICommand ShuffleRecentListensCommand { get; }

    public ICommand PlaySimilarArtistSection0Command { get; }

    public ICommand ShuffleSimilarArtistSection0Command { get; }

    public ICommand PlaySimilarArtistSection1Command { get; }

    public ICommand ShuffleSimilarArtistSection1Command { get; }

    public ICommand PlaySimilarArtistSection2Command { get; }

    public ICommand ShuffleSimilarArtistSection2Command { get; }

    public ICommand PlaySimilarArtistSection3Command { get; }

    public ICommand ShuffleSimilarArtistSection3Command { get; }

    public ICommand PlaySimilarArtistSection4Command { get; }

    public ICommand ShuffleSimilarArtistSection4Command { get; }

    #endregion

    #region Construction

    public HomeViewModel(
        MusicAssistantService musicAssistant,
        UserDataService userDataService,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<HomeViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playbackService = playbackService;
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

            await _playbackService.PlayMediaAsync(new List<MediaItem> { track });
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

        ShowPlaylistContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_playlistContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, anchor);
            }
        });

        ShowPlaylistContextMenuAtPositionCommand = new Command<Point>(async position =>
        {
            if (_playlistContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_playlistContextMenuItems, position);
            }
        });

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

        _ = BuildTrackContextMenuAsync();
        _ = BuildPlaylistContextMenuAsync();
        _ = BuildArtistContextMenuAsync();

        PlayArtistPlaylistsCommand = new Command(async () => await PlayArtistPlaylistsAsync());

        ShuffleArtistPlaylistsCommand = new Command(async () => await ShuffleArtistPlaylistsAsync());

        PlayRecommendationTracksCommand = new Command(async () => await PlayCollectionFirstAsync(RecommendationTracks));

        ShuffleRecommendationTracksCommand = new Command(async () => await PlayCollectionRandomAsync(RecommendationTracks));

        PlayGenrePlaylistsCommand = new Command(async () => await PlayCollectionFirstAsync(GenrePlaylists));

        ShuffleGenrePlaylistsCommand = new Command(async () => await PlayCollectionRandomAsync(GenrePlaylists));

        PlayTopTracksCommand = new Command(async () =>
        {
            var tracks = TopTracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await _playbackService.PlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        ShuffleTopTracksCommand = new Command(async () =>
        {
            var tracks = TopTracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await _playbackService.ShufflePlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        PlayTopArtistsCommand = new Command(async () => await PlayCollectionFirstAsync(TopArtists));

        ShuffleTopArtistsCommand = new Command(async () => await PlayCollectionRandomAsync(TopArtists));

        PlayRecentListensCommand = new Command(async () => await PlayCollectionFirstAsync(RecentListens));

        ShuffleRecentListensCommand = new Command(async () => await PlayCollectionRandomAsync(RecentListens));

        PlaySimilarArtistSection0Command = new Command(async () => await PlaySimilarArtistSectionByIndexAsync(0));

        ShuffleSimilarArtistSection0Command = new Command(async () => await ShuffleSimilarArtistSectionByIndexAsync(0));

        PlaySimilarArtistSection1Command = new Command(async () => await PlaySimilarArtistSectionByIndexAsync(1));

        ShuffleSimilarArtistSection1Command = new Command(async () => await ShuffleSimilarArtistSectionByIndexAsync(1));

        PlaySimilarArtistSection2Command = new Command(async () => await PlaySimilarArtistSectionByIndexAsync(2));

        ShuffleSimilarArtistSection2Command = new Command(async () => await ShuffleSimilarArtistSectionByIndexAsync(2));

        PlaySimilarArtistSection3Command = new Command(async () => await PlaySimilarArtistSectionByIndexAsync(3));

        ShuffleSimilarArtistSection3Command = new Command(async () => await ShuffleSimilarArtistSectionByIndexAsync(3));

        PlaySimilarArtistSection4Command = new Command(async () => await PlaySimilarArtistSectionByIndexAsync(4));

        ShuffleSimilarArtistSection4Command = new Command(async () => await ShuffleSimilarArtistSectionByIndexAsync(4));
    }

    #endregion

    #region INavigationAware

    public Task OnNavigatedToAsync(object? parameter)
    {
        _logger.LogDebug("Loading ListenBrainz recommendation folder for home");
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
        SetSectionLoadingState(true);

        try
        {
            // Get all listenbrainz recommendation folders
            var recommendations = await _musicAssistant.GetRecommendationsAsync();
            var lbFolders = recommendations
                .Where(IsListenBrainzFolder)
                .ToList();

            // First render wave: load recommendations and genre playlists in parallel.
            var recommendationTask = LoadRecommendationTracksAsync(lbFolders);
            var genrePlaylistsTask = LoadGenrePlaylistsAsync(lbFolders);
            await Task.WhenAll(recommendationTask, genrePlaylistsTask);
            IsLoadingHome = false;

            // Then load remaining sections sequentially to avoid heavy simultaneous UI updates.
            await LoadArtistPlaylistsAsync(lbFolders);
            await LoadTopTracksAsync(lbFolders);
            await LoadTopArtistsAsync(lbFolders);
            await LoadSimilarArtistsSectionsAsync(lbFolders);
            await LoadRecentListensAsync();
        }
        catch (UnauthorizedAccessException ex)
        {
            _logger.LogWarning(ex, "Home loading requires authentication. Triggering login flow.");
            ResetHomeSectionsAfterFailure();
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home sections");
            ResetHomeSectionsAfterFailure();
        }
        finally
        {
            SetSectionLoadingState(false);
            if (IsLoadingHome)
            {
                IsLoadingHome = false;
            }
        }
    }

    private void ResetHomeSectionsAfterFailure()
    {
        RecommendationTracks = new ObservableRangeCollection<Track>();
        TopTracks = new ObservableRangeCollection<Track>();
        RecentListens = new ObservableRangeCollection<Track>();
        TopArtists = new ObservableRangeCollection<Artist>();
        SimilarArtistSections = new ObservableRangeCollection<SimilarArtistSection>();
        GenrePlaylists = new ObservableRangeCollection<Playlist>();
        ArtistPlaylists = new ObservableRangeCollection<Playlist>();
        SetSectionLoadingState(false);
        IsLoadingHome = false;
    }

    private async Task LoadRecommendationTracksAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        IsLoadingRecommendationTracks = true;

        try
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

            await ApplyFavoriteStateAsync(recommendationTracks);

            // Shuffle recommendations on each load so the slide order varies.
            for (var i = recommendationTracks.Count - 1; i > 0; i--)
            {
                var swapIndex = Random.Shared.Next(i + 1);
                (recommendationTracks[i], recommendationTracks[swapIndex]) = (recommendationTracks[swapIndex], recommendationTracks[i]);
            }

            RecommendationTracks = new ObservableRangeCollection<Track>(recommendationTracks);
            await Task.Delay(50);

            _logger.LogDebug(
                "Home recommendation folder loaded: recommendationTracks={RecommendationTracks}",
                recommendationTracks.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home recommendation folder");
            RecommendationTracks = new ObservableRangeCollection<Track>();
        }
        finally
        {
            IsLoadingRecommendationTracks = false;
        }
    }

    private async Task LoadTopTracksAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        IsLoadingTopTracks = true;
        try
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

            await ApplyFavoriteStateAsync(topTracks);

            TopTracks = new ObservableRangeCollection<Track>(topTracks);
            await Task.Delay(50);

            _logger.LogDebug(
                "Home top tracks loaded: topTracks={TopTracks}",
                topTracks.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home top tracks");
            TopTracks = new ObservableRangeCollection<Track>();
        }
        finally
        {
            IsLoadingTopTracks = false;
        }
    }

    private async Task LoadRecentListensAsync()
    {
        IsLoadingRecentListens = true;
        try
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

            await ApplyFavoriteStateAsync(recentTracks);

            RecentListens = new ObservableRangeCollection<Track>(recentTracks);
            await Task.Delay(50);

            _logger.LogDebug(
                "Home recent listens loaded: recentTracks={RecentTracks}, userId={UserId}",
                recentTracks.Count,
                currentUser?.UserId ?? string.Empty);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home recent listens");
            RecentListens = new ObservableRangeCollection<Track>();
        }
        finally
        {
            IsLoadingRecentListens = false;
        }
    }

    private async Task LoadTopArtistsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        IsLoadingTopArtists = true;
        try
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

            await ApplyFavoriteStateAsync(topArtists);

            TopArtists = new ObservableRangeCollection<Artist>(topArtists);
            await Task.Delay(50);

            _logger.LogDebug(
                "Home top artists loaded: topArtists={TopArtists}",
                topArtists.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home top artists");
            TopArtists = new ObservableRangeCollection<Artist>();
        }
        finally
        {
            IsLoadingTopArtists = false;
        }
    }

    private async Task LoadSimilarArtistsSectionsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        IsLoadingSimilarArtistSections = true;
        try
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

                await ApplyFavoriteStateAsync(artists);

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
            await Task.Delay(50);

            _logger.LogDebug(
                "Home similar artists sections loaded: similarArtistSections={SimilarArtistSections}",
                similarArtistSections.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home similar artist sections");
            SimilarArtistSections = new ObservableRangeCollection<SimilarArtistSection>();
        }
        finally
        {
            IsLoadingSimilarArtistSections = false;
        }
    }

    private async Task LoadGenrePlaylistsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        IsLoadingGenrePlaylists = true;
        try
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
                    playlist.Owner = null;
                    return playlist;
                })
                .Take(9)
                .ToList()
                ?? new List<Playlist>();

            await ApplyFavoriteStateAsync(genrePlaylists);

            GenrePlaylists = new ObservableRangeCollection<Playlist>(genrePlaylists);
            await Task.Delay(50);

            _logger.LogDebug(
                "Home genre playlists loaded: genrePlaylists={GenrePlaylists}",
                genrePlaylists.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home genre playlists");
            GenrePlaylists = new ObservableRangeCollection<Playlist>();
        }
        finally
        {
            IsLoadingGenrePlaylists = false;
        }
    }

    private async Task LoadArtistPlaylistsAsync(IEnumerable<RecommendationFolder> lbFolders)
    {
        IsLoadingArtistPlaylists = true;
        try
        {
            var artistPlaylists = FindRecommendationFolderById(lbFolders, "artist_playlists")?.Items?
                .OfType<Playlist>()
                .Where(playlist => !string.IsNullOrWhiteSpace(playlist.ItemId) && !string.IsNullOrWhiteSpace(playlist.Provider))
                .Select(playlist =>
                {
                    playlist.DisplayName = NormalizePlaylistDisplayName(playlist.Name);
                    playlist.Owner = null;
                    return playlist;
                })
                .ToList()
                ?? new List<Playlist>();

            await ApplyFavoriteStateAsync(artistPlaylists);

            ArtistPlaylists = new ObservableRangeCollection<Playlist>(artistPlaylists);
            await Task.Delay(50);

            _logger.LogDebug(
                "Home artist playlists loaded: artistPlaylists={ArtistPlaylists}",
                artistPlaylists.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home artist playlists");
            ArtistPlaylists = new ObservableRangeCollection<Playlist>();
        }
        finally
        {
            IsLoadingArtistPlaylists = false;
        }
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
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await _playbackService.PlayMediaAsync(GetSelectedTracks().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var targetTracks = GetSelectedTracks()
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
                    await _playbackService.PlayMediaAsync(targetItems);
                    await _playbackService.PlayMediaRadioNextAsync(targetItems);

                    var duplicateIndex = targetTracks.Count;
                    string? duplicateQueueItemId = null;
                    for (var attempt = 0; attempt < 10; attempt++)
                    {
                        if (_playbackService.CurrentQueueItems.Count > duplicateIndex)
                        {
                            duplicateQueueItemId = _playbackService.CurrentQueueItems[duplicateIndex].QueueItemId;
                            if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
                            {
                                break;
                            }
                        }

                        await Task.Delay(500);
                    }

                    if (!string.IsNullOrWhiteSpace(duplicateQueueItemId))
                    {
                        await _playbackService.DeleteQueueItemAsync(duplicateQueueItemId);
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
                    await _playbackService.PlayMediaNextAsync(GetSelectedTracks().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _playbackService.PlayMediaLastAsync(GetSelectedTracks().Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedTracks().Cast<MediaItem>().ToList();
                    await _userDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedTracks().Cast<MediaItem>().ToList();
                    await _userDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
            }
        };

        return Task.CompletedTask;
    }

    private Task BuildPlaylistContextMenuAsync()
    {
        _playlistContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () => await _playbackService.PlayMediaAsync(GetSelectedPlaylists().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _playbackService.PlayMediaNextAsync(GetSelectedPlaylists().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _playbackService.PlayMediaLastAsync(GetSelectedPlaylists().Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedPlaylists().Cast<MediaItem>().ToList();
                    await _userDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedPlaylists().Cast<MediaItem>().ToList();
                    await _userDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
            }
        };

        return Task.CompletedTask;
    }

    private Task BuildArtistContextMenuAsync()
    {
        _artistContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () => await _playbackService.PlayMediaAsync(GetSelectedArtists().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var targetArtists = GetSelectedArtists().ToList();
                    if (targetArtists.Count == 0)
                    {
                        _logger.LogDebug("Cannot start artist radio: no target artists available.");
                        return;
                    }

                    var radioArtist = targetArtists[Random.Shared.Next(targetArtists.Count)];
                    var topTracks = await _musicAssistant.GetArtistTopTracksAsync(radioArtist.ItemId, radioArtist.Provider);
                    if (topTracks.Count == 0)
                    {
                        _logger.LogDebug("Cannot start artist radio: no top tracks available for selected artist {ArtistId}.", radioArtist.ItemId);
                        return;
                    }

                    var randomTopTrack = topTracks[Random.Shared.Next(topTracks.Count)];
                    await _playbackService.PlayMediaAsync(new List<MediaItem> { randomTopTrack });
                    await _playbackService.PlayMediaRadioNextAsync(new List<MediaItem> { radioArtist });
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _playbackService.PlayMediaNextAsync(GetSelectedArtists().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _playbackService.PlayMediaLastAsync(GetSelectedArtists().Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedArtists().Cast<MediaItem>().ToList();
                    await _userDataService.SetFavoriteAsync(selectedMediaItems, true);
                })
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    var selectedMediaItems = GetSelectedArtists().Cast<MediaItem>().ToList();
                    await _userDataService.SetFavoriteAsync(selectedMediaItems, false);
                })
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
        if (IsLoadingSimilarArtistSections)
        {
            return _rowSkeletons.Count;
        }

        return GetSimilarArtistSection(index)?.Artists.Count ?? 0;
    }

    private bool GetShowSimilarArtistSectionContainer(int index)
    {
        return IsLoadingSimilarArtistSections || GetSimilarArtistSection(index) != null;
    }

    private bool GetShowSimilarArtistSectionRowView(int index)
    {
        return IsLoadingSimilarArtistSections || (GetSimilarArtistSection(index)?.Artists.Count > 0);
    }

    private bool GetShowNoSimilarArtistSectionMessage(int index)
    {
        var section = GetSimilarArtistSection(index);
        return !IsLoadingSimilarArtistSections && section != null && section.Artists.Count == 0;
    }

    private IEnumerable<object> GetSimilarArtistSectionItems(int index)
    {
        if (IsLoadingSimilarArtistSections)
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

    private void SetSectionLoadingState(bool value)
    {
        IsLoadingRecommendationTracks = value;
        IsLoadingTopTracks = value;
        IsLoadingRecentListens = value;
        IsLoadingTopArtists = value;
        IsLoadingSimilarArtistSections = value;
        IsLoadingGenrePlaylists = value;
        IsLoadingArtistPlaylists = value;
    }


    private IEnumerable<Track> GetSelectedTracks()
    {
        return RecommendationTracks
            .Concat(TopTracks)
            .Concat(RecentListens)
            .Where(track => track.IsSelected)
            .ToList();
    }

    private IEnumerable<Playlist> GetSelectedPlaylists()
    {
        return GenrePlaylists
            .Concat(ArtistPlaylists)
            .Where(playlist => playlist.IsSelected)
            .ToList();
    }

    private IEnumerable<Artist> GetSelectedArtists()
    {
        return TopArtists
            .Concat(SimilarArtistSections.SelectMany(section => section.Artists))
            .Where(artist => artist.IsSelected)
            .ToList();
    }

    private async Task PlayArtistPlaylistsAsync()
    {
        await PlayCollectionFirstAsync(ArtistPlaylists);
    }

    private async Task ShuffleArtistPlaylistsAsync()
    {
        await PlayCollectionRandomAsync(ArtistPlaylists);
    }

    private async Task PlaySimilarArtistSectionByIndexAsync(int index)
    {
        var sectionArtists = GetSimilarArtistSection(index)?.Artists;
        if (sectionArtists == null)
        {
            return;
        }

        await PlayCollectionFirstAsync(sectionArtists);
    }

    private async Task ShuffleSimilarArtistSectionByIndexAsync(int index)
    {
        var sectionArtists = GetSimilarArtistSection(index)?.Artists;
        if (sectionArtists == null)
        {
            return;
        }

        await PlayCollectionRandomAsync(sectionArtists);
    }

    private async Task PlayCollectionFirstAsync<T>(IEnumerable<T>? items) where T : MediaItem
    {
        if (items == null)
        {
            return;
        }

        var firstItem = items.FirstOrDefault();
        if (firstItem == null)
        {
            return;
        }

        await _playbackService.PlayMediaAsync(new List<MediaItem> { firstItem });
    }

    private async Task PlayCollectionRandomAsync<T>(IEnumerable<T>? items) where T : MediaItem
    {
        if (items == null)
        {
            return;
        }

        var entries = items.Where(entry => entry != null).ToList();
        if (entries.Count == 0)
        {
            return;
        }

        var randomIndex = Random.Shared.Next(entries.Count);
        await _playbackService.PlayMediaAsync(new List<MediaItem> { entries[randomIndex] });
    }

    private async Task ApplyFavoriteStateAsync<T>(IEnumerable<T> items) where T : MediaItem
    {
        if (items == null)
        {
            return;
        }

        foreach (var item in items)
        {
            item.Favorite = await _userDataService.IsFavoriteAsync(item);
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
        _topTracks.Clear();
        _recentListens.Clear();
        _topArtists.Clear();
        _similarArtistSections.Clear();
        _genrePlaylists.Clear();
        _artistPlaylists.Clear();
        _trackContextMenuItems.Clear();
        _playlistContextMenuItems.Clear();
        _artistContextMenuItems.Clear();

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

