using mashin.Collections;
using mashin.Models;
using mashin.Services;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.ApplicationModel;
using Microsoft.Maui.Controls;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Globalization;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Windows.Input;

namespace mashin.ViewModels;

public class ArtistDetailViewModel : INotifyPropertyChanged, INavigationAware, IDisposable
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IPlaylistService _playlistService;
    private readonly IContextMenuService _contextMenuService;
    private readonly INavigationService _navigationService;
    private readonly ILogger<ArtistDetailViewModel> _logger;
    private readonly Random _shuffleRandom = new();
    private static readonly HttpClient DeezerHttpClient = new()
    {
        Timeout = TimeSpan.FromSeconds(20)
    };

    private Artist? _artist;
    private List<Album> _allAlbums = new();
    private ObservableRangeCollection<Track> _topTracks = new();
    private ObservableRangeCollection<ContextMenuItem> _headerContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _trackContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _albumContextMenuItems = new();
    private ObservableRangeCollection<ContextMenuItem> _artistContextMenuItems = new();
    private ObservableRangeCollection<Album> _albums = new();
    private ObservableRangeCollection<Artist> _similarArtists = new();

    private readonly IReadOnlyList<TableViewSkeleton> _trackSkeletons = Enumerable.Range(0, 15)
        .Select(_ => new TableViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _albumSkeletons = Enumerable.Range(0, 15)
        .Select(_ => new RowViewSkeleton())
        .ToList();
    private readonly IReadOnlyList<RowViewSkeleton> _artistSkeletons = Enumerable.Range(0, 15)
        .Select(_ => new RowViewSkeleton())
        .ToList();

    private bool _isLoading;
    private bool _isLoadingMetadata;
    private bool _isLoadingAlbums;
    private bool _isLoadingTracks;
    private bool _isLoadingSimilarArtists;
    private bool _isDescriptionExpanded;
    private bool _disposed;
    private Track? _contextMenuTargetTrack;
    private Artist? _contextMenuTargetArtist;
    #endregion

    #region Properties

    public Artist? Artist
    {
        get => _artist;
        set
        {
            if (SetProperty(ref _artist, value))
            {
                OnPropertyChanged(nameof(ArtistName));
                OnPropertyChanged(nameof(ImageUri));
                OnPropertyChanged(nameof(HasDescription));
                OnPropertyChanged(nameof(IsArtistFavorite));
                IsDescriptionExpanded = false;
            }
        }
    }

    public string ArtistName => Artist?.Name ?? "Unbekannter Interpret";

    public string? ImageUri => Artist?.ImageUri;

    public bool HasDescription => !string.IsNullOrWhiteSpace(Artist?.Metadata?.Description);

    public bool IsArtistFavorite => Artist?.Favorite ?? false;

    public ObservableRangeCollection<Track> TopTracks
    {
        get => _topTracks;
        set
        {
            if (SetProperty(ref _topTracks, value))
            {
                OnPropertyChanged(nameof(TopTrackItems));
            }
        }
    }

    public ObservableRangeCollection<Album> Albums
    {
        get => _albums;
        set
        {
            if (SetProperty(ref _albums, value))
            {
                OnPropertyChanged(nameof(AlbumItems));
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
    public ICommand PlayAlbumsCommand { get; }
    public ICommand ShuffleAlbumsCommand { get; }
    public ICommand PlaySimilarArtistsCommand { get; }
    public ICommand ShuffleSimilarArtistsCommand { get; }
    public ICommand PlayArtistCommand { get; }
    public ICommand ShuffleArtistCommand { get; }
    public ICommand StartArtistRadioCommand { get; }
    public ICommand ToggleArtistFavoriteCommand { get; }
    public ICommand ToggleDescriptionCommand { get; }

    public bool IsLoading
    {
        get => _isLoading;
        set => SetProperty(ref _isLoading, value);
    }

    public bool IsLoadingMetadata
    {
        get => _isLoadingMetadata;
        set => SetProperty(ref _isLoadingMetadata, value);
    }

    public bool IsLoadingAlbums
    {
        get => _isLoadingAlbums;
        set
        {
            if (SetProperty(ref _isLoadingAlbums, value))
            {
                OnPropertyChanged(nameof(AlbumItems));
            }
        }
    }

    public bool IsLoadingTracks
    {
        get => _isLoadingTracks;
        set
        {
            if (SetProperty(ref _isLoadingTracks, value))
            {
                OnPropertyChanged(nameof(TopTrackItems));
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
                OnPropertyChanged(nameof(MobileDescriptionMaxLines));
            }
        }
    }

    public int DescriptionMaxLines => IsDescriptionExpanded ? int.MaxValue : 4;

    public int MobileDescriptionMaxLines => IsDescriptionExpanded ? int.MaxValue : 3;

    public IEnumerable<object> TopTrackItems => IsLoadingTracks ? _trackSkeletons : _topTracks;
    public IEnumerable<object> AlbumItems => IsLoadingAlbums ? _albumSkeletons : _albums;
    public IEnumerable<object> SimilarArtistItems => IsLoadingSimilarArtists ? _artistSkeletons : _similarArtists;

    #endregion

    #region Construction

    public ArtistDetailViewModel(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IPlaylistService playlistService,
        IMediaItemActions mediaActions,
        PlaybackService playbackService,
        IContextMenuService contextMenuService,
        INavigationService navigationService,
        ILogger<ArtistDetailViewModel> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _playlistService = playlistService;
        _contextMenuService = contextMenuService;
        _navigationService = navigationService;
        _logger = logger;

        MediaActions = mediaActions;
        PlaybackService = playbackService;

        // Navigation Commands
        AlbumTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<AlbumDetailPage>(parameter));

        ArtistTappedCommand = new Command<object>(async parameter => await _navigationService.NavigateToAsync<ArtistDetailPage>(parameter));

        // Playback Commands
        PlayAlbumsCommand = new Command(async () =>
        {
            var albums = Albums.ToList();
            if (albums.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(new List<MediaItem> { albums[0] });
        });

        ShuffleAlbumsCommand = new Command(async () =>
        {
            var albums = Albums.ToList();
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

        PlayArtistCommand = new Command(async () =>
        {
            var topTracks = TopTracks.ToList();
            if (topTracks.Count == 0)
            {
                return;
            }

            await PlaybackService.PlayMediaAsync(topTracks.Cast<MediaItem>().ToList());
        });

        ShuffleArtistCommand = new Command(async () =>
        {
            if (Artist == null)
            {
                return;
            }

            var tracks = TopTracks.ToList();
            if (tracks.Count == 0)
            {
                return;
            }

            await PlaybackService.ShufflePlayMediaAsync(tracks.Cast<MediaItem>().ToList());
        });

        StartArtistRadioCommand = new Command(async () =>
        {
            if (Artist == null)
            {
                return;
            }

            var topTracks = TopTracks.ToList();
            if (topTracks.Count == 0)
            {
                _logger.LogDebug("Cannot start artist radio: no top tracks available for current artist.");
                return;
            }

            var randomTopTrack = topTracks[_shuffleRandom.Next(topTracks.Count)];
            await PlaybackService.PlayMediaAsync(new List<MediaItem> { randomTopTrack });

            await PlaybackService.PlayMediaRadioNextAsync(new List<MediaItem> { Artist });
        });

        ToggleArtistFavoriteCommand = new Command(async () =>
        {
            if (Artist == null)
            {
                return;
            }

            if (Artist.Favorite)
            {
                await MediaActions.RemoveFromFavoritesAsync(Artist);
            }
            else
            {
                await MediaActions.AddToFavoritesAsync(Artist);
            }

            OnPropertyChanged(nameof(IsArtistFavorite));
            await BuildHeaderContextMenuAsync();
        });

        // Toggle Commands
        ToggleDescriptionCommand = new Command(() => IsDescriptionExpanded = !IsDescriptionExpanded);

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

        ShowTrackContextMenuAtAnchorCommand = new Command<View>(async (anchor) =>
        {
            if (anchor == null)
            {
                return;
            }

            _contextMenuTargetTrack = anchor.BindingContext as Track;
            if (_trackContextMenuItems.Count > 0)
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
            if (anchor == null)
            {
                return;
            }

            _contextMenuTargetArtist = anchor.BindingContext as Artist;

            if (_artistContextMenuItems.Count > 0)
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
            _logger.LogDebug("Navigated to artist target: {ItemId} ({Provider})", item.ItemId, item.Provider);

            _ = LoadArtistAsync(item.ItemId, item.Provider);
        }
        else
        {
            _logger.LogWarning("NavigatedTo called without valid MediaItem parameter");
        }

        return Task.CompletedTask;
    }

    public Task OnNavigatedFromAsync()
    {
        _logger.LogDebug("Navigated away from artist: {ArtistName}", ArtistName);
        return Task.CompletedTask;
    }

    #endregion

    #region Data Loading

    // Loads artist details progressively
    public async Task LoadArtistAsync(string artistId, string providerInstanceOrDomain = "library")
    {
        IsLoading = true;
        IsLoadingMetadata = true;
        IsLoadingAlbums = true;
        IsLoadingTracks = true;
        IsLoadingSimilarArtists = true;
        
        try
        {
            // Artist Metadata 
            await LoadArtistMetadataAsync(artistId, providerInstanceOrDomain);

            // Albums
            await LoadAlbumsAsync(artistId, providerInstanceOrDomain);

            // Top Tracks
            await LoadTopTracksAsync(artistId, providerInstanceOrDomain);

            // Similar Artists
            await LoadSimilarArtistsAsync(artistId, providerInstanceOrDomain);

            
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load artist: {ArtistId}", artistId);
        }
        finally
        {
            IsLoading = false;
        }
    }

    private async Task LoadArtistMetadataAsync(string artistId, string provider)
    {
        IsLoadingMetadata = true;
        try
        {
            Artist = await _musicAssistant.GetArtistAsync(artistId, provider);
            if (Artist != null)
            {
                // Load deezer bio if deezer provider
                if (provider.StartsWith("deezer", StringComparison.OrdinalIgnoreCase) && string.IsNullOrWhiteSpace(Artist.Metadata?.Description))
                {
                    var language = CultureInfo.CurrentUICulture.TwoLetterISOLanguageName;
                    var deezerBio = await GetDeezerArtistBio(artistId, language);
                    if (!string.IsNullOrWhiteSpace(deezerBio))
                    {
                        Artist.Metadata ??= new MediaItemMetadata();
                        Artist.Metadata.Description = deezerBio;

                        OnPropertyChanged(nameof(Artist));
                        OnPropertyChanged(nameof(HasDescription));
                    }
                }

                // Set favorite state
                Artist.Favorite = await _userDataService.IsFavoriteAsync(Artist);
                OnPropertyChanged(nameof(IsArtistFavorite));
            }

            if (Artist == null)
            {
                _logger.LogWarning("Artist not found: {ArtistId}", artistId);
            }

            _ = BuildHeaderContextMenuAsync();
        }
        finally
        {
            IsLoadingMetadata = false;
        }
    }

    private async Task<string?> GetDeezerArtistBio(string artistId, string language)
    {
        var normalizedLanguage = string.IsNullOrWhiteSpace(language) ? "de" : language.Trim().ToLowerInvariant();

        try
        {
            // Get anonymous JWT token from deezer (required for artist bio request)
            var loginRequest = new HttpRequestMessage(HttpMethod.Get, "https://auth.deezer.com/login/anonymous?jo=p&rto=c");
            loginRequest.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
            loginRequest.Headers.TryAddWithoutValidation("accept-language", normalizedLanguage);
            loginRequest.Headers.Referrer = new Uri("https://www.deezer.com/");
            loginRequest.Headers.UserAgent.ParseAdd("Mozilla/5.0");

            using var loginResponse = await DeezerHttpClient.SendAsync(loginRequest);
            if (!loginResponse.IsSuccessStatusCode)
            {
                _logger.LogDebug("Deezer anonymous login failed with status code {StatusCode}", loginResponse.StatusCode);
                return null;
            }

            var loginJson = await loginResponse.Content.ReadAsStringAsync();
            using var loginDocument = JsonDocument.Parse(loginJson);
            if (!loginDocument.RootElement.TryGetProperty("jwt", out var jwtElement))
            {
                return null;
            }

            var jwt = jwtElement.GetString();
            if (string.IsNullOrWhiteSpace(jwt))
            {
                return null;
            }

            // Get artist bio using GraphQL API
            var graphqlPayload = new
            {
                operationName = "ArtistBio",
                variables = new { artistId },
                query = "query ArtistBio($artistId: String!) { artist(artistId: $artistId) { id name bio { full } } }"
            };

            var gqlRequest = new HttpRequestMessage(HttpMethod.Post, "https://pipe.deezer.com/api")
            {
                Content = new StringContent(
                    JsonSerializer.Serialize(graphqlPayload),
                    Encoding.UTF8,
                    "application/json")
            };

            gqlRequest.Headers.Authorization = new AuthenticationHeaderValue("Bearer", jwt);
            gqlRequest.Headers.TryAddWithoutValidation("origin", "https://www.deezer.com");
            gqlRequest.Headers.Referrer = new Uri("https://www.deezer.com/");
            gqlRequest.Headers.TryAddWithoutValidation("accept-language", normalizedLanguage);

            using var gqlResponse = await DeezerHttpClient.SendAsync(gqlRequest);
            if (!gqlResponse.IsSuccessStatusCode)
            {
                _logger.LogDebug("Deezer ArtistBio request failed with status code {StatusCode}", gqlResponse.StatusCode);
                return null;
            }

            var gqlJson = await gqlResponse.Content.ReadAsStringAsync();
            using var gqlDocument = JsonDocument.Parse(gqlJson);

            if (gqlDocument.RootElement.TryGetProperty("data", out var dataElement)
                && dataElement.TryGetProperty("artist", out var artistElement)
                && artistElement.TryGetProperty("bio", out var bioElement)
                && bioElement.TryGetProperty("full", out var fullElement))
            {
                var bio = fullElement.GetString();

                if (string.IsNullOrWhiteSpace(bio))
                {
                    return null;
                }

                // Deezer bios often contain HTML tags and entities, so we need to clean that up
                var text = bio;
                text = Regex.Replace(text, "<\\s*br\\s*/?\\s*>", "\n", RegexOptions.IgnoreCase);
                text = Regex.Replace(text, "<\\s*/\\s*p\\s*>", "\n\n", RegexOptions.IgnoreCase);
                text = Regex.Replace(text, "<\\s*p[^>]*>", string.Empty, RegexOptions.IgnoreCase);

                text = Regex.Replace(text, "<[^>]+>", string.Empty);
                text = WebUtility.HtmlDecode(text);

                text = text.Replace("\r\n", "\n").Replace("\r", "\n");
                text = Regex.Replace(text, "[ \t]+\n", "\n");
                text = Regex.Replace(text, "\n{3,}", "\n\n");
                //text = AddParagraphs(text);

                text = text.Trim();
                return string.IsNullOrWhiteSpace(text) ? null : text;
            }

            return null;
        }
        catch (Exception ex)
        {
            _logger.LogDebug(ex, "Failed to fetch Deezer artist bio for artist {ArtistId}", artistId);
            return null;
        }
    }

    private async Task LoadAlbumsAsync(string artistId, string provider)
    {
        IsLoadingAlbums = true;
        try
        {
            // Get all albums and sort by year descending
            var albums = await _musicAssistant.GetArtistAlbumsAsync(artistId, provider);
            var sortedAlbums = albums.OrderByDescending(a => a.Year ?? 0).ToList();

            _allAlbums = sortedAlbums;

            // Load albums
            Albums = new ObservableRangeCollection<Album>(_allAlbums);
            IsLoadingAlbums = false;        
            
            _ = BuildAlbumContextMenuAsync();

        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load albums");
            IsLoadingAlbums = false;
        }
    }

    private async Task LoadTopTracksAsync(string artistId, string provider)
    {
        IsLoadingTracks = true;
        try
        {
            var tracks = await _musicAssistant.GetArtistTopTracksAsync(artistId, provider);

            var processedTracks = new List<Track>();
            for (var i = 0; i < tracks.Count; i++)
            {
                tracks[i].Index = i;

                // try to fill missing album year from already loaded albums
                var album = tracks[i].Album;
                if (album != null && !album.Year.HasValue && Albums.Count > 0)
                {
                    var albumWithYear = _allAlbums.FirstOrDefault(a => a.ItemId == album.ItemId) ??
                                       _allAlbums.FirstOrDefault(a => string.Equals(a.Name, album.Name, StringComparison.OrdinalIgnoreCase));

                    if (albumWithYear?.Year.HasValue == true)
                    {
                        album.Year = albumWithYear.Year;
                    }
                }

                processedTracks.Add(tracks[i]);
            }

            TopTracks = new ObservableRangeCollection<Track>(processedTracks);
            IsLoadingTracks = false;
            await Task.Delay(50);
            

            _ = BuildTrackContextMenuAsync();

        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to load tracks");
            IsLoadingTracks = false;
        }
    }

    private async Task LoadSimilarArtistsAsync(string artistId, string provider)
    {
        IsLoadingSimilarArtists = true;
        try
        {
            var similarArtists = await _musicAssistant.GetSimilarArtistsAsync(
                artistId,
                provider,
                limit: 25);

            // similar_tracks fallback
            if (similarArtists.Count == 0)
            {
                var topTrack = TopTracks.FirstOrDefault();
                var similarTracks = topTrack != null
                    ? await _musicAssistant.GetSimilarTracksAsync(
                        topTrack.ItemId,
                        topTrack.Provider,
                        limit: 50,
                        allowLookup: true)
                    : new List<Track>();

                if (topTrack != null && similarTracks.Count == 0)
                {
                    var versions = await _musicAssistant.GetTrackVersionsAsync(topTrack.ItemId, topTrack.Provider);
                    var fallbackVersion = versions
                        .FirstOrDefault(v => !string.Equals(v.Provider, topTrack.Provider, StringComparison.OrdinalIgnoreCase))
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

                similarArtists = similarTracks
                    .SelectMany(track => track.Artists ?? Enumerable.Empty<Artist>())
                    .GroupBy(artist => string.Concat(artist.Provider, "|", artist.ItemId))
                    .Select(group => group.First())
                    .Where(artist => artist.ItemId != artistId)
                    .Take(25)
                    .ToList();

                if (similarArtists.Count > 0)
                {
                    var enrichedArtists = await Task.WhenAll(similarArtists.Select(async artist =>
                    {
                        try
                        {
                            var fullArtist = await _musicAssistant.GetArtistAsync(artist.ItemId, artist.Provider);
                            return fullArtist ?? artist;
                        }
                        catch (Exception ex)
                        {
                            _logger.LogDebug(ex, "Failed to enrich similar fallback artist: {ArtistId} ({Provider})", artist.ItemId, artist.Provider);
                            return artist;
                        }
                    }));

                    similarArtists = enrichedArtists.ToList();
                }
            }

            SimilarArtists = new ObservableRangeCollection<Artist>(similarArtists);

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
        if (Artist == null)
        {
            _headerContextMenuItems = new ObservableRangeCollection<ContextMenuItem>();
            return Task.CompletedTask;
        }

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    if (Artist == null)
                    {
                        return;
                    }

                    var topTracks = TopTracks.ToList();
                    if (topTracks.Count == 0)
                    {
                        _logger.LogDebug("Cannot start artist radio: no top tracks available for current artist.");
                        return;
                    }

                    var randomTopTrack = topTracks[_shuffleRandom.Next(topTracks.Count)];
                    await PlaybackService.PlayMediaAsync(new List<MediaItem> { randomTopTrack });

                    await PlaybackService.PlayMediaRadioNextAsync(new List<MediaItem> { Artist });
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = new List<MediaItem> { Artist! };
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = new List<MediaItem> { Artist! };
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true }
        };

        if (Artist.Favorite)
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                {
                    await MediaActions.RemoveFromFavoritesAsync(Artist);
                    OnPropertyChanged(nameof(IsArtistFavorite));
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
                    await MediaActions.AddToFavoritesAsync(Artist);
                    OnPropertyChanged(nameof(IsArtistFavorite));
                    await BuildHeaderContextMenuAsync();
                })
            });
        }

        _headerContextMenuItems = menu;
        return Task.CompletedTask;
    }

    private Task BuildTrackContextMenuAsync()
    {
        var playlists = _playlistService.Playlists;

        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaAsync(GetContextMenuTargetTracks().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var targetTracks = GetContextMenuTargetTracks()
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
                    await PlaybackService.PlayMediaNextAsync(GetContextMenuTargetTracks().Cast<MediaItem>().ToList()))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                    await PlaybackService.PlayMediaLastAsync(GetContextMenuTargetTracks().Cast<MediaItem>().ToList()))
            },
            new() { IsSeparator = true },
            new()
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
                                await MediaActions.AddToPlaylistAsync(
                                    GetContextMenuTargetTracks(),
                                    playlist))
                        }))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(GetContextMenuTargetTracks()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(GetContextMenuTargetTracks()))
            }
        };

        _trackContextMenuItems = menu;
        return Task.CompletedTask;
    }

    private IReadOnlyList<Track> GetContextMenuTargetTracks()
    {
        var selectedTracks = TopTracks.Where(track => track.IsSelected).ToList();
        if (selectedTracks.Count > 0)
        {
            return selectedTracks;
        }

        return _contextMenuTargetTrack == null ? Array.Empty<Track>() : new[] { _contextMenuTargetTrack };
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
                {
                    var items = Albums
                        .Where(a => a.IsSelected)
                        .Select(a => (MediaItem)a)
                        .ToList();
                    await PlaybackService.PlayMediaAsync(items);
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = Albums
                        .Where(a => a.IsSelected)
                        .Select(a => (MediaItem)a)
                        .ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = Albums
                        .Where(a => a.IsSelected)
                        .Select(a => (MediaItem)a)
                        .ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(Albums.Where(a => a.IsSelected)))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(Albums.Where(a => a.IsSelected)))
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
                {
                    var items = GetContextMenuTargetArtists()
                        .Select(a => (MediaItem)a)
                        .ToList();
                    await PlaybackService.PlayMediaAsync(items);
                })
            },
            new()
            {
                Text = "Radio starten",
                Icon = FluentIcons.Album20,
                Command = new Command(async () =>
                {
                    var targetArtists = GetContextMenuTargetArtists()
                        .Where(a => a != null && !string.IsNullOrWhiteSpace(a.ItemId) && !string.IsNullOrWhiteSpace(a.Provider))
                        .DistinctBy(a => string.Concat(a.Provider, "|", a.ItemId))
                        .ToList();

                    if (targetArtists.Count == 0)
                    {
                        _logger.LogDebug("Cannot start artist radio: no target artists available.");
                        return;
                    }

                    var radioArtist = targetArtists[_shuffleRandom.Next(targetArtists.Count)];

                    List<Track> topTracks;
                    if (Artist != null
                        && string.Equals(Artist.ItemId, radioArtist.ItemId, StringComparison.Ordinal)
                        && string.Equals(Artist.Provider, radioArtist.Provider, StringComparison.Ordinal)
                        && TopTracks.Count > 0)
                    {
                        topTracks = TopTracks.ToList();
                    }
                    else
                    {
                        topTracks = await _musicAssistant.GetArtistTopTracksAsync(radioArtist.ItemId, radioArtist.Provider);
                    }

                    if (topTracks.Count == 0)
                    {
                        _logger.LogDebug("Cannot start artist radio: no top tracks available for selected artist {ArtistId}.", radioArtist.ItemId);
                        return;
                    }

                    var randomTopTrack = topTracks[_shuffleRandom.Next(topTracks.Count)];
                    await PlaybackService.PlayMediaAsync(new List<MediaItem> { randomTopTrack });

                    await PlaybackService.PlayMediaRadioNextAsync(new List<MediaItem> { radioArtist });
                })
            },
            new()
            {
                Text = "Als Nächstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () =>
                {
                    var items = GetContextMenuTargetArtists()
                        .Select(a => (MediaItem)a)
                        .ToList();
                    await PlaybackService.PlayMediaNextAsync(items);
                })
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () =>
                {
                    var items = GetContextMenuTargetArtists()
                        .Select(a => (MediaItem)a)
                        .ToList();
                    await PlaybackService.PlayMediaLastAsync(items);
                })
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Favoriten hinzufügen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () =>
                    await MediaActions.AddToFavoritesAsync(GetContextMenuTargetArtists()))
            },
            new()
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () =>
                    await MediaActions.RemoveFromFavoritesAsync(GetContextMenuTargetArtists()))
            }
        };

        _artistContextMenuItems = menu;
        return Task.CompletedTask;
    }

    private IReadOnlyList<Artist> GetContextMenuTargetArtists()
    {
        var selectedArtists = SimilarArtists.Where(artist => artist.IsSelected).ToList();
        if (selectedArtists.Count > 0)
        {
            return selectedArtists;
        }

        return _contextMenuTargetArtist == null ? Array.Empty<Artist>() : new[] { _contextMenuTargetArtist };
    }

    #endregion

    #region INotifyPropertyChanged

    public event PropertyChangedEventHandler? PropertyChanged;

    protected void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        if (propertyName == nameof(IsLoadingMetadata)
            || propertyName == nameof(IsLoadingAlbums))
        {
            _navigationService.IsNavigating = IsLoadingMetadata || IsLoadingAlbums;
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

        _allAlbums.Clear();
        _topTracks.Clear();
        _headerContextMenuItems.Clear();
        _trackContextMenuItems.Clear();
        _albumContextMenuItems.Clear();
        _artistContextMenuItems.Clear();
        _albums.Clear();
        _similarArtists.Clear();
        PropertyChanged = null;
    }

    #endregion

        #region Helper Methods

    private static string AddParagraphs(string text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return string.Empty;
        }

        var normalized = text.Replace("\r\n", "\n").Replace("\r", "\n");
        normalized = Regex.Replace(normalized, "[ \t]+", " ").Trim();

        var sentences = Regex.Split(normalized, @"(?<=[.!?])\s+")
            .Where(part => !string.IsNullOrWhiteSpace(part))
            .ToList();

        if (sentences.Count <= 1)
        {
            return normalized;
        }

        const int maxParagraphLength = 600;
        var paragraphs = new List<string>();
        var currentParagraph = new StringBuilder();

        foreach (var sentence in sentences)
        {
            if (currentParagraph.Length > 0
                && currentParagraph.Length + 1 + sentence.Length > maxParagraphLength)
            {
                paragraphs.Add(currentParagraph.ToString().Trim());
                currentParagraph.Clear();
            }

            if (currentParagraph.Length > 0)
            {
                currentParagraph.Append(' ');
            }

            currentParagraph.Append(sentence.Trim());
        }

        if (currentParagraph.Length > 0)
        {
            paragraphs.Add(currentParagraph.ToString().Trim());
        }

        return string.Join("\n\n", paragraphs);
    }

    #endregion
}
