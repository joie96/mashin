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
    private readonly IUserDataService _userDataService;
    private readonly IMediaItemActions _mediaActions;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly IQueueSyncService _queueSyncService;
    private readonly ILogger<HomeViewModel> _logger;
    private readonly Random _shuffleRandom = new();

    private ObservableRangeCollection<Playlist> _genrePlaylists = new();
    private ObservableRangeCollection<Playlist> _favoritePlaylists = new();
    private ObservableRangeCollection<Track> _mixTracks = new();
    private ObservableRangeCollection<Playlist> _artistPlaylists = new();
    private ObservableRangeCollection<Artist> _topArtists = new();
    private ObservableRangeCollection<Track> _recommendationTracks = new();
    private ObservableRangeCollection<HomeArtistSection> _genreArtistSections = new();
    private ObservableRangeCollection<HomeArtistSection> _similarArtistSections = new();

    private ObservableRangeCollection<ContextMenuItem> _playlistContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _mixTrackContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _trackContextMenuItems = new();

    private readonly IReadOnlyList<RowViewSkeleton> _rowSkeletons = Enumerable.Range(0, 20)
        .Select(_ => new RowViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<TableViewSkeleton> _tableSkeletons = Enumerable.Range(0, 10)
        .Select(_ => new TableViewSkeleton())
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

    private bool _isLoadingHome;
    private bool _disposed;

    #endregion

    #region Properties

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
                OnPropertyChanged(nameof(ShowGenrePlaylists));
                OnPropertyChanged(nameof(ShowNoGenrePlaylistsMessage));
                OnPropertyChanged(nameof(GenrePlaylistItems));
            }
        }
    }

    public ObservableRangeCollection<Playlist> FavoritePlaylists
    {
        get => _favoritePlaylists;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Playlist>();
            if (!EqualityComparer<ObservableRangeCollection<Playlist>>.Default.Equals(_favoritePlaylists, normalizedValue))
            {
                _favoritePlaylists = normalizedValue;
                OnPropertyChanged(nameof(FavoritePlaylists));
                OnPropertyChanged(nameof(HasFavoritePlaylists));
                OnPropertyChanged(nameof(ShowFavoritePlaylists));
                OnPropertyChanged(nameof(ShowNoFavoritePlaylistsMessage));
                OnPropertyChanged(nameof(FavoritePlaylistItems));
            }
        }
    }

    public ObservableRangeCollection<Track> MixTracks
    {
        get => _mixTracks;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<Track>();
            if (!EqualityComparer<ObservableRangeCollection<Track>>.Default.Equals(_mixTracks, normalizedValue))
            {
                _mixTracks = normalizedValue;
                OnPropertyChanged(nameof(MixTracks));
                OnPropertyChanged(nameof(HasMixTracks));
                OnPropertyChanged(nameof(ShowMixTracks));
                OnPropertyChanged(nameof(ShowNoMixTracksMessage));
                OnPropertyChanged(nameof(MixTrackItems));
            }
        }
    }

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
                OnPropertyChanged(nameof(ShowArtistPlaylists));
                OnPropertyChanged(nameof(ShowNoArtistPlaylistsMessage));
                OnPropertyChanged(nameof(ArtistPlaylistItems));
            }
        }
    }

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
                OnPropertyChanged(nameof(ShowTopArtists));
                OnPropertyChanged(nameof(ShowNoTopArtistsMessage));
                OnPropertyChanged(nameof(TopArtistItems));
            }
        }
    }

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
                OnPropertyChanged(nameof(HasRecommendationTracks));
                OnPropertyChanged(nameof(ShowRecommendationTracks));
                OnPropertyChanged(nameof(ShowNoRecommendationTracksMessage));
                OnPropertyChanged(nameof(RecommendationTrackItems));
            }
        }
    }

    public ObservableRangeCollection<HomeArtistSection> GenreArtistSections
    {
        get => _genreArtistSections;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<HomeArtistSection>();
            if (!EqualityComparer<ObservableRangeCollection<HomeArtistSection>>.Default.Equals(_genreArtistSections, normalizedValue))
            {
                _genreArtistSections = normalizedValue;
                OnPropertyChanged(nameof(GenreArtistSections));
                OnPropertyChanged(nameof(HasGenreArtistSections));
                OnPropertyChanged(nameof(ShowGenreArtistSections));
                OnPropertyChanged(nameof(ShowNoGenreArtistSectionsMessage));
            }
        }
    }

    public ObservableRangeCollection<HomeArtistSection> SimilarArtistSections
    {
        get => _similarArtistSections;
        private set
        {
            var normalizedValue = value ?? new ObservableRangeCollection<HomeArtistSection>();
            if (!EqualityComparer<ObservableRangeCollection<HomeArtistSection>>.Default.Equals(_similarArtistSections, normalizedValue))
            {
                _similarArtistSections = normalizedValue;
                OnPropertyChanged(nameof(SimilarArtistSections));
                OnPropertyChanged(nameof(HasSimilarArtistSections));
                OnPropertyChanged(nameof(ShowSimilarArtistSections));
                OnPropertyChanged(nameof(ShowNoSimilarArtistSectionsMessage));
            }
        }
    }

    public bool HasGenrePlaylists => GenrePlaylists.Count > 0;
    public bool HasFavoritePlaylists => FavoritePlaylists.Count > 0;
    public bool HasMixTracks => MixTracks.Count > 0;
    public bool HasArtistPlaylists => ArtistPlaylists.Count > 0;
    public bool HasTopArtists => TopArtists.Count > 0;
    public bool HasRecommendationTracks => RecommendationTracks.Count > 0;
    public bool HasGenreArtistSections => GenreArtistSections.Count > 0;
    public bool HasSimilarArtistSections => SimilarArtistSections.Count > 0;

    public bool ShowGenrePlaylists => IsLoadingHome || HasGenrePlaylists;
    public bool ShowFavoritePlaylists => IsLoadingHome || HasFavoritePlaylists;
    public bool ShowMixTracks => IsLoadingHome || HasMixTracks;
    public bool ShowArtistPlaylists => IsLoadingHome || HasArtistPlaylists;
    public bool ShowTopArtists => IsLoadingHome || HasTopArtists;
    public bool ShowRecommendationTracks => IsLoadingHome || HasRecommendationTracks;
    public bool ShowGenreArtistSections => !IsLoadingHome && HasGenreArtistSections;
    public bool ShowSimilarArtistSections => !IsLoadingHome && HasSimilarArtistSections;

    public bool ShowNoGenrePlaylistsMessage => !IsLoadingHome && !HasGenrePlaylists;
    public bool ShowNoFavoritePlaylistsMessage => !IsLoadingHome && !HasFavoritePlaylists;
    public bool ShowNoMixTracksMessage => !IsLoadingHome && !HasMixTracks;
    public bool ShowNoArtistPlaylistsMessage => !IsLoadingHome && !HasArtistPlaylists;
    public bool ShowNoTopArtistsMessage => !IsLoadingHome && !HasTopArtists;
    public bool ShowNoRecommendationTracksMessage => !IsLoadingHome && !HasRecommendationTracks;
    public bool ShowNoGenreArtistSectionsMessage => !IsLoadingHome && !HasGenreArtistSections;
    public bool ShowNoSimilarArtistSectionsMessage => !IsLoadingHome && !HasSimilarArtistSections;

    public IEnumerable<object> GenrePlaylistItems => IsLoadingHome ? _rowSkeletons : _genrePlaylists;
    public IEnumerable<object> FavoritePlaylistItems => IsLoadingHome ? _rowSkeletons : _favoritePlaylists;
    public IEnumerable<object> MixTrackItems => IsLoadingHome ? _rowSkeletons : _mixTracks;
    public IEnumerable<object> ArtistPlaylistItems => IsLoadingHome ? _rowSkeletons : _artistPlaylists;
    public IEnumerable<object> TopArtistItems => IsLoadingHome ? _rowSkeletons : _topArtists;
    public IEnumerable<object> RecommendationTrackItems => IsLoadingHome ? _tableSkeletons : _recommendationTracks;

    public bool IsLoadingHome
    {
        get => _isLoadingHome;
        private set
        {
            if (SetProperty(ref _isLoadingHome, value))
            {
                OnPropertyChanged(nameof(ShowGenrePlaylists));
                OnPropertyChanged(nameof(ShowFavoritePlaylists));
                OnPropertyChanged(nameof(ShowMixTracks));
                OnPropertyChanged(nameof(ShowArtistPlaylists));
                OnPropertyChanged(nameof(ShowTopArtists));
                OnPropertyChanged(nameof(ShowRecommendationTracks));
                OnPropertyChanged(nameof(ShowGenreArtistSections));
                OnPropertyChanged(nameof(ShowSimilarArtistSections));

                OnPropertyChanged(nameof(ShowNoGenrePlaylistsMessage));
                OnPropertyChanged(nameof(ShowNoFavoritePlaylistsMessage));
                OnPropertyChanged(nameof(ShowNoMixTracksMessage));
                OnPropertyChanged(nameof(ShowNoArtistPlaylistsMessage));
                OnPropertyChanged(nameof(ShowNoTopArtistsMessage));
                OnPropertyChanged(nameof(ShowNoRecommendationTracksMessage));
                OnPropertyChanged(nameof(ShowNoGenreArtistSectionsMessage));
                OnPropertyChanged(nameof(ShowNoSimilarArtistSectionsMessage));

                OnPropertyChanged(nameof(GenrePlaylistItems));
                OnPropertyChanged(nameof(FavoritePlaylistItems));
                OnPropertyChanged(nameof(MixTrackItems));
                OnPropertyChanged(nameof(ArtistPlaylistItems));
                OnPropertyChanged(nameof(TopArtistItems));
                OnPropertyChanged(nameof(RecommendationTrackItems));
            }
        }
    }

    public IMediaItemActions MediaActions => _mediaActions;

    public ICommand AlbumTappedCommand { get; }
    public ICommand ArtistTappedCommand { get; }
    public ICommand PlaylistTappedCommand { get; }
    public ICommand MixTrackTappedCommand { get; }

    public ICommand ShowPlaylistContextMenuAtAnchorCommand { get; }
    public ICommand ShowPlaylistContextMenuAtPositionCommand { get; }
    public ICommand ShowMixTrackContextMenuAtAnchorCommand { get; }
    public ICommand ShowMixTrackContextMenuAtPositionCommand { get; }
    public ICommand ShowArtistContextMenuAtAnchorCommand { get; }
    public ICommand ShowArtistContextMenuAtPositionCommand { get; }
    public ICommand ShowTrackContextMenuAtAnchorCommand { get; }
    public ICommand ShowTrackContextMenuAtPositionCommand { get; }

    #endregion

    #region Construction

    public HomeViewModel(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IMediaItemActions mediaActions,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        IQueueSyncService queueSyncService,
        ILogger<HomeViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _mediaActions = mediaActions;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _queueSyncService = queueSyncService;
        _logger = logger;

        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));
        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));
        PlaylistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<PlaylistDetailPage>(parameter));

        MixTrackTappedCommand = new Command<object>(async parameter =>
        {
            if (parameter is not Track track)
            {
                return;
            }

            await PlayMixTrackWithShuffleAsync(track);
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

        ShowMixTrackContextMenuAtAnchorCommand = new Command<View>(async anchor =>
        {
            if (_mixTrackContextMenuItems.Count > 0 && anchor != null)
            {
                await _contextMenuService.ShowContextMenuAsync(_mixTrackContextMenuItems, anchor);
            }
        });

        ShowMixTrackContextMenuAtPositionCommand = new Command<Point>(async position =>
        {
            if (_mixTrackContextMenuItems.Count > 0)
            {
                await _contextMenuService.ShowContextMenuAsync(_mixTrackContextMenuItems, position);
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
        _logger.LogInformation("Loading home recommendations");
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
            await _userDataService.GetPreferencesAsync();
            var recommendations = await _musicAssistant.GetRecommendationsAsync();
            var lbFolders = recommendations
                .Where(IsListenBrainzFolder)
                .ToList();

            var genrePlaylists = ParsePlaylistsFromRecommendationFolder(
                FindRecommendationFolderById(lbFolders, "genre_playlists"));
            var artistPlaylists = ParsePlaylistsFromRecommendationFolder(
                FindRecommendationFolderById(lbFolders, "artist_playlists"));
            var topArtists = ParseArtistsFromRecommendationFolder(
                FindRecommendationFolderById(lbFolders, "top_artists"));
            var recommendationTracks = ParseTracksFromRecommendationFolder(
                FindRecommendationFolderById(lbFolders, "recommendations"));
            var topTracks = ParseTracksFromRecommendationFolder(
                FindRecommendationFolderById(lbFolders, "top_tracks"));

            var favoritePlaylists = await LoadFavoritePlaylistsAsync();

            var genreArtistSections = BuildArtistSections(lbFolders, "genre_artists_", "100% ");
            var similarArtistSections = BuildArtistSections(lbFolders, "similar_artists_", "Ähnlich wie ");

            await _musicAssistant.EnrichWithProviderInfoAsync(genrePlaylists);
            await _musicAssistant.EnrichWithProviderInfoAsync(artistPlaylists);
            await _musicAssistant.EnrichWithProviderInfoAsync(favoritePlaylists);
            await _musicAssistant.EnrichWithProviderInfoAsync(topArtists);
            await _musicAssistant.EnrichWithProviderInfoAsync(recommendationTracks);
            await _musicAssistant.EnrichWithProviderInfoAsync(topTracks);

            var genreArtists = genreArtistSections.SelectMany(section => section.Artists).ToList();
            var similarArtists = similarArtistSections.SelectMany(section => section.Artists).ToList();
            await _musicAssistant.EnrichWithProviderInfoAsync(genreArtists);
            await _musicAssistant.EnrichWithProviderInfoAsync(similarArtists);

            ApplyPlaylistDisplayNames(genrePlaylists);
            ApplyPlaylistDisplayNames(favoritePlaylists);
            ApplyPlaylistDisplayNames(artistPlaylists);

            GenrePlaylists = new ObservableRangeCollection<Playlist>(genrePlaylists);
            FavoritePlaylists = new ObservableRangeCollection<Playlist>(favoritePlaylists);
            MixTracks = new ObservableRangeCollection<Track>(topTracks);
            ArtistPlaylists = new ObservableRangeCollection<Playlist>(artistPlaylists);
            TopArtists = new ObservableRangeCollection<Artist>(topArtists);
            RecommendationTracks = new ObservableRangeCollection<Track>(recommendationTracks);
            GenreArtistSections = new ObservableRangeCollection<HomeArtistSection>(genreArtistSections);
            SimilarArtistSections = new ObservableRangeCollection<HomeArtistSection>(similarArtistSections);

            _ = BuildPlaylistContextMenuAsync();
            _ = BuildMixTrackContextMenuAsync();
            _ = BuildArtistContextMenuAsync();
            _ = BuildTrackContextMenuAsync();

            _logger.LogInformation(
                "Home recommendations loaded: genrePlaylists={GenrePlaylists}, favoritePlaylists={FavoritePlaylists}, mixTracks={MixTracks}, artistPlaylists={ArtistPlaylists}, topArtists={TopArtists}, recommendationTracks={RecommendationTracks}, genreArtistSections={GenreSections}, similarArtistSections={SimilarSections}",
                genrePlaylists.Count,
                favoritePlaylists.Count,
                topTracks.Count,
                artistPlaylists.Count,
                topArtists.Count,
                recommendationTracks.Count,
                genreArtistSections.Count,
                similarArtistSections.Count);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to load home recommendations");

            GenrePlaylists = new ObservableRangeCollection<Playlist>();
            FavoritePlaylists = new ObservableRangeCollection<Playlist>();
            MixTracks = new ObservableRangeCollection<Track>();
            ArtistPlaylists = new ObservableRangeCollection<Playlist>();
            TopArtists = new ObservableRangeCollection<Artist>();
            RecommendationTracks = new ObservableRangeCollection<Track>();
            GenreArtistSections = new ObservableRangeCollection<HomeArtistSection>();
            SimilarArtistSections = new ObservableRangeCollection<HomeArtistSection>();
        }
        finally
        {
            IsLoadingHome = false;
        }
    }

    #endregion

    #region Context Menu

    private Task BuildPlaylistContextMenuAsync()
    {
        _playlistContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaAsync(GetSelectedPlaylists()))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaNextAsync(GetSelectedPlaylists()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaLastAsync(GetSelectedPlaylists()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await _mediaActions.AddToFavoritesAsync(GetSelectedPlaylists()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await _mediaActions.RemoveFromFavoritesAsync(GetSelectedPlaylists()))
            }
        };

        return Task.CompletedTask;
    }

    private Task BuildMixTrackContextMenuAsync()
    {
        _mixTrackContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen (Shuffle)",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaySelectedMixTracksWithShuffleAsync())
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaNextAsync(GetSelectedMixTracks()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaLastAsync(GetSelectedMixTracks()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await _mediaActions.AddToFavoritesAsync(GetSelectedMixTracks()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await _mediaActions.RemoveFromFavoritesAsync(GetSelectedMixTracks()))
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
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaAsync(GetSelectedArtists()))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaNextAsync(GetSelectedArtists()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaLastAsync(GetSelectedArtists()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await _mediaActions.AddToFavoritesAsync(GetSelectedArtists()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await _mediaActions.RemoveFromFavoritesAsync(GetSelectedArtists()))
            }
        };

        return Task.CompletedTask;
    }

    private Task BuildTrackContextMenuAsync()
    {
        _trackContextMenuItems = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaySelectedRecommendationsWithModesAsync())
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaNextAsync(GetSelectedRecommendationTracks()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await _mediaActions.PlayMediaLastAsync(GetSelectedRecommendationTracks()))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await _mediaActions.AddToFavoritesAsync(GetSelectedRecommendationTracks()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await _mediaActions.RemoveFromFavoritesAsync(GetSelectedRecommendationTracks()))
            }
        };

        return Task.CompletedTask;
    }

    #endregion

    #region Helpers

    private async Task<List<Playlist>> LoadFavoritePlaylistsAsync()
    {
        var snapshot = await _userDataService.GetFavoritesSnapshotAsync();
        if (snapshot == null || snapshot.Playlists.Count == 0)
        {
            return new List<Playlist>();
        }

        var playlists = snapshot.Playlists
            .Select(BuildPlaylistFromSnapshot)
            .ToList();

        for (var i = playlists.Count - 1; i > 0; i--)
        {
            var j = _shuffleRandom.Next(i + 1);
            (playlists[i], playlists[j]) = (playlists[j], playlists[i]);
        }

        return playlists;
    }

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

    private static List<HomeArtistSection> BuildArtistSections(
        IEnumerable<RecommendationFolder> folders,
        string itemIdPrefix,
        string titlePrefix)
    {
        return folders
            .Where(folder => folder.ItemId.StartsWith(itemIdPrefix, StringComparison.OrdinalIgnoreCase))
            .Select(folder =>
            {
                var title = BuildSectionTitle(titlePrefix, folder);
                var artists = ParseArtistsFromRecommendationFolder(folder);
                return new HomeArtistSection(title, artists);
            })
            .Where(section => section.Artists.Count > 0)
            .ToList();
    }

    private static string BuildSectionTitle(string prefix, RecommendationFolder folder)
    {
        var name = folder.Name?.Trim();
        if (string.IsNullOrWhiteSpace(name))
        {
            return prefix.TrimEnd();
        }

        return $"{prefix}{name}";
    }

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

    private static List<Playlist> ParsePlaylistsFromRecommendationFolder(RecommendationFolder? folder)
    {
        if (folder?.Items == null || folder.Items.Count == 0)
        {
            return new List<Playlist>();
        }

        var playlists = new List<Playlist>();

        foreach (var item in folder.Items)
        {
            if (item.ValueKind != JsonValueKind.Object)
            {
                continue;
            }

            if (!item.TryGetProperty("media_type", out var mediaTypeProperty)
                || !string.Equals(mediaTypeProperty.GetString(), "playlist", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var playlist = JsonSerializer.Deserialize<Playlist>(item.GetRawText(), RecommendationItemJsonOptions);
            if (playlist == null || string.IsNullOrWhiteSpace(playlist.ItemId) || string.IsNullOrWhiteSpace(playlist.Provider))
            {
                continue;
            }

            playlist.DisplayName = playlist.Name;
            playlists.Add(playlist);
        }

        return playlists;
    }

    private static List<Track> ParseTracksFromRecommendationFolder(RecommendationFolder? folder)
    {
        if (folder?.Items == null || folder.Items.Count == 0)
        {
            return new List<Track>();
        }

        var tracks = new List<Track>();

        foreach (var item in folder.Items)
        {
            if (item.ValueKind != JsonValueKind.Object)
            {
                continue;
            }

            if (!item.TryGetProperty("media_type", out var mediaTypeProperty)
                || !string.Equals(mediaTypeProperty.GetString(), "track", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var track = JsonSerializer.Deserialize<Track>(item.GetRawText(), RecommendationItemJsonOptions);
            if (track == null || string.IsNullOrWhiteSpace(track.ItemId) || string.IsNullOrWhiteSpace(track.Provider))
            {
                continue;
            }

            track.DisplayName = track.Name;
            tracks.Add(track);
        }

        return tracks;
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
        var username = _userDataService.CurrentUser?.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
    }

    private static Playlist BuildPlaylistFromSnapshot(FavoritePlaylistSnapshot snapshot)
    {
        var playlist = new Playlist
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Favorite = true,
            ProviderMappings = snapshot.ProviderMappings?.ToList() ?? new List<ProviderMapping>()
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            playlist.DisplayName = snapshot.DisplayName;
        }

        playlist.Metadata = BuildMetadata(snapshot.ImageUrl);
        return playlist;
    }

    private static MediaItemMetadata? BuildMetadata(string? imageUrl)
    {
        if (string.IsNullOrWhiteSpace(imageUrl))
        {
            return null;
        }

        return new MediaItemMetadata
        {
            Images = new List<MediaItemImage>
            {
                new()
                {
                    Type = "thumb",
                    Path = imageUrl,
                    Provider = "builtin"
                }
            }
        };
    }

    private IEnumerable<Playlist> GetSelectedPlaylists()
    {
        return GenrePlaylists
            .Concat(FavoritePlaylists)
            .Concat(ArtistPlaylists)
            .Where(playlist => playlist.IsSelected)
            .ToList();
    }

    private IEnumerable<Track> GetSelectedMixTracks()
    {
        return MixTracks
            .Where(mixTrack => mixTrack.IsSelected)
            .ToList();
    }

    private IEnumerable<Artist> GetSelectedArtists()
    {
        var selected = new List<Artist>();
        selected.AddRange(TopArtists.Where(artist => artist.IsSelected));

        foreach (var section in GenreArtistSections)
        {
            selected.AddRange(section.Artists.Where(artist => artist.IsSelected));
        }

        foreach (var section in SimilarArtistSections)
        {
            selected.AddRange(section.Artists.Where(artist => artist.IsSelected));
        }

        return selected;
    }

    private IEnumerable<Track> GetSelectedRecommendationTracks()
    {
        return RecommendationTracks.Where(track => track.IsSelected).ToList();
    }

    private async Task PlaySelectedMixTracksWithShuffleAsync()
    {
        var selectedTracks = GetSelectedMixTracks().ToList();
        if (selectedTracks.Count == 0)
        {
            return;
        }

        if (selectedTracks.Count == 1)
        {
            await PlayMixTrackWithShuffleAsync(selectedTracks[0]);
            return;
        }

        await _mediaActions.PlayMediaAsync(selectedTracks[0]);
        await _mediaActions.PlayMediaNextAsync(selectedTracks.Skip(1).ToList());
        await SetShuffleEnabledAsync();
    }

    private async Task PlaySelectedRecommendationsWithModesAsync()
    {
        var trackList = RecommendationTracks.ToList();
        var selectedTracks = trackList.Where(track => track.IsSelected).ToList();
        if (selectedTracks.Count == 0)
        {
            return;
        }

        if (selectedTracks.Count > 1)
        {
            await _mediaActions.PlayMediaAsync(selectedTracks[0]);
            await _mediaActions.PlayMediaNextAsync(selectedTracks.Skip(1).ToList());
            return;
        }

        var selectedTrack = selectedTracks[0];
        await _mediaActions.PlayMediaAsync(selectedTrack);

        var selectedIndex = trackList.IndexOf(selectedTrack);
        if (selectedIndex < 0 || trackList.Count <= 1)
        {
            return;
        }

        var queueCandidates = new List<Track>(trackList.Count - 1);
        var trailingCount = trackList.Count - selectedIndex - 1;
        if (trailingCount > 0)
        {
            queueCandidates.AddRange(trackList.GetRange(selectedIndex + 1, trailingCount));
        }

        if (selectedIndex > 0)
        {
            queueCandidates.AddRange(trackList.GetRange(0, selectedIndex));
        }

        if (queueCandidates.Count > 0)
        {
            await _mediaActions.PlayMediaNextAsync(queueCandidates);
        }
    }

    private async Task PlayMixTrackWithShuffleAsync(Track track)
    {
        if (track == null)
        {
            return;
        }

        await _mediaActions.PlayMediaAsync(track);
        await SetShuffleEnabledAsync();
    }

    private async Task SetShuffleEnabledAsync()
    {
        await _queueSyncService.RefreshNowAsync();

        var queueId = _queueSyncService.CurrentPlayerQueue?.QueueId;
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetShuffleAsync(queueId, true);
        await _queueSyncService.RefreshNowAsync();
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

        _genrePlaylists.Clear();
        _favoritePlaylists.Clear();
        _mixTracks.Clear();
        _artistPlaylists.Clear();
        _topArtists.Clear();
        _recommendationTracks.Clear();
        _genreArtistSections.Clear();
        _similarArtistSections.Clear();

        _playlistContextMenuItems.Clear();
        _mixTrackContextMenuItems.Clear();
        _artistContextMenuItems.Clear();
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

public sealed class HomeArtistSection
{
    public HomeArtistSection(string title, IEnumerable<Artist> artists)
    {
        Title = title;
        Artists = new ObservableRangeCollection<Artist>(artists?.ToList() ?? new List<Artist>());
    }

    public string Title { get; }
    public ObservableRangeCollection<Artist> Artists { get; }
    public IEnumerable<object> ArtistItems => Artists.Cast<object>();
}
