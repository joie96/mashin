using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.Extensions.Logging;
using mashin.Models;
using mashin.Converters;

namespace mashin.Services;

/// <summary>
/// Music Assistant API Service - Complete implementation of the Music Assistant API
/// </summary>
public class MusicAssistantService
{
    private static readonly SocketsHttpHandler SharedHttpHandler = new()
    {
        AutomaticDecompression = DecompressionMethods.GZip | DecompressionMethods.Deflate | DecompressionMethods.Brotli,
        PooledConnectionIdleTimeout = TimeSpan.FromMinutes(2),
        PooledConnectionLifetime = TimeSpan.FromMinutes(10),
        MaxConnectionsPerServer = 16,
        ConnectTimeout = TimeSpan.FromSeconds(10),
        UseCookies = false,
        EnableMultipleHttp2Connections = true
    };

    private readonly ILogger<MusicAssistantService> _logger;
    private readonly SettingsService _settings;
    private readonly HttpClient _httpClient;

    // Authentication state
    private string? _authToken;
    private bool _isAuthenticated;

    // Provider Manifest Cache
    private readonly Dictionary<string, ProviderManifest> _providerManifestCache = new();
    private readonly SemaphoreSlim _manifestCacheLock = new(1, 1);

    // JSON Serializer options with custom converters
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        Converters =
        {
            new JsonStringEnumConverter(),
            new FlexibleIntConverter(),
            new MediaItemJsonConverter()
        }
    };

    public MusicAssistantService(
        ILogger<MusicAssistantService> logger,
        SettingsService settings)
    {
        _logger = logger;
        _settings = settings;

        _httpClient = new HttpClient(SharedHttpHandler, disposeHandler: false)
        {
            Timeout = TimeSpan.FromSeconds(60),
            BaseAddress = new Uri(settings.MusicAssistantUrl),
            DefaultRequestVersion = HttpVersion.Version20,
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrLower
        };

        _authToken = _settings.AuthToken;
        _isAuthenticated = !string.IsNullOrWhiteSpace(_authToken);
    }

    public bool IsAuthenticated => _isAuthenticated;

    public string? AuthToken => _authToken;

    public event EventHandler? LoginRequired;

    private async Task<T?> SendCommandAsync<T>(string command, object? args = null)
    {
        SetAuthHeader();

        try
        {
            var payload = new
            {
                command = command,
                args = args ?? new { }
            };

            var json = JsonSerializer.Serialize(payload, JsonOptions);
            _logger.LogDebug("Sending command: {Command}", command);

            var content = new StringContent(json, Encoding.UTF8, "application/json");
            using var response = await _httpClient.PostAsync("/api", content);

            if (response.StatusCode == System.Net.HttpStatusCode.Unauthorized)
            {
                _logger.LogWarning("Unauthorized - token may have expired");
                Logout();
                RequestLogin();
                throw new UnauthorizedAccessException("Authentication token expired or invalid");
            }

            response.EnsureSuccessStatusCode();

            var responseJson = await response.Content.ReadAsStringAsync();
            _logger.LogTrace("Response: {Response}", responseJson);

            return JsonSerializer.Deserialize<T>(responseJson, JsonOptions);
        }
        catch (HttpRequestException ex)
        {
            _logger.LogError(ex, "HTTP error sending command: {Command}", command);
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to send command: {Command}", command);
            throw;
        }
    }

    #region Authentication

    /// <summary>
    /// Authenticate user with credentials via WebSocket. This command allows clients to authenticate over the WebSocket connection using username/password or other provider-specific credentials.
    /// </summary>
    public async Task<bool> AutoLoginAsync(bool raiseLoginRequest = true)
    {
        if (!string.IsNullOrWhiteSpace(_settings.AuthToken))
        {
            _logger.LogInformation("Attempting auto-login with saved token...");
            SetAuthSession(_settings.AuthToken!, _settings.Username);

            try
            {
                using var request = new HttpRequestMessage(HttpMethod.Post, "/api");
                request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", _settings.AuthToken!);

                var payload = JsonSerializer.Serialize(new
                {
                    command = "info",
                    args = new { }
                });

                request.Content = new StringContent(payload, Encoding.UTF8, "application/json");

                using var response = await _httpClient.SendAsync(request);
                response.EnsureSuccessStatusCode();

                _logger.LogInformation("Auto-login successful");
                return true;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Saved token invalid, clearing...");
                Logout();
            }
        }

        _logger.LogInformation("No valid auth token available, login required");
        if (raiseLoginRequest)
        {
            RequestLogin();
        }

        return false;
    }

    public async Task<bool> LoginAsync(string username, string password, string? deviceName = null)
    {
        try
        {
            _logger.LogInformation("Authenticating with Music Assistant...");

            var args = new Dictionary<string, object>
            {
                ["username"] = username,
                ["password"] = password,
                ["provider_id"] = "builtin"
            };

            if (!string.IsNullOrEmpty(deviceName))
            {
                args["device_name"] = deviceName;
            }

            var response = await SendCommandAsync<AuthResponse>("auth/login", args);

            if (response?.Success == true && !string.IsNullOrEmpty(response.Token))
            {
                SetAuthSession(response.Token, response.User?.Username ?? username);

                _logger.LogInformation("Successfully authenticated as: {Username}", response.User?.Username ?? username);

                return true;
            }

            return false;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Login failed");
            return false;
        }
    }

    /// <summary>
    /// Logout current user by revoking the current token.
    /// </summary>
    public async Task LogoutAsync()
    {
        try
        {
            if (IsAuthenticated)
            {
                await SendCommandAsync<object>("auth/logout");
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Logout command failed, clearing local credentials anyway");
        }
        finally
        {
            Logout();
        }
    }

    /// <summary>
    /// Get current authenticated user information.
    /// </summary>
    public async Task<AuthUser?> GetCurrentUserAsync()
    {
        return await SendCommandAsync<AuthUser>("auth/me");
    }

    /// <summary>
    /// Get list of available authentication providers. Returns information about all available login providers including whether they require OAuth redirect flow.
    /// </summary>
    public async Task<List<object>?> GetAuthProvidersAsync()
    {
        return await SendCommandAsync<List<object>>("auth/providers");
    }

    /// <summary>
    /// Create a new long-lived access token for current user or another user (admin only). Long-lived tokens are intended for external integrations and API access. They expire after 10 years and do NOT auto-renew on use.
    /// </summary>
    public async Task<string?> CreateTokenAsync(string name, string? userId = null)
    {
        var args = new Dictionary<string, object>
        {
            ["name"] = name
        };

        if (!string.IsNullOrEmpty(userId))
        {
            args["user_id"] = userId;
        }

        return await SendCommandAsync<string>("auth/token/create", args);
    }

    /// <summary>
    /// Revoke an auth token.
    /// </summary>
    public async Task RevokeTokenAsync(string tokenId)
    {
        await SendCommandAsync<object>("auth/token/revoke", new { token_id = tokenId });
    }

    /// <summary>
    /// Update user profile information. Users can update their own profile. Admins can update any user including role, password and filters.
    /// </summary>
    public async Task<AuthUser?> UpdateUserAsync(
        string? userId = null,
        string? username = null,
        string? displayName = null,
        string? avatarUrl = null,
        string? password = null,
        string? role = null,
        Dictionary<string, object>? preferences = null,
        List<string>? playerFilter = null,
        List<string>? providerFilter = null)
    {
        if (!string.IsNullOrEmpty(password) && password.Length < 8)
        {
            throw new ArgumentException("Password must be at least 8 characters.", nameof(password));
        }

        if (!string.IsNullOrWhiteSpace(role) && role != "admin" && role != "user")
        {
            throw new ArgumentException("Role must be either 'admin' or 'user'.", nameof(role));
        }

        var args = new Dictionary<string, object>();

        if (!string.IsNullOrWhiteSpace(userId))
        {
            args["user_id"] = userId;
        }

        if (!string.IsNullOrWhiteSpace(username))
        {
            args["username"] = username;
        }

        if (!string.IsNullOrWhiteSpace(displayName))
        {
            args["display_name"] = displayName;
        }

        if (!string.IsNullOrWhiteSpace(avatarUrl))
        {
            args["avatar_url"] = avatarUrl;
        }

        if (!string.IsNullOrEmpty(password))
        {
            args["password"] = password;
        }

        if (!string.IsNullOrWhiteSpace(role))
        {
            args["role"] = role;
        }

        if (preferences is not null)
        {
            args["preferences"] = preferences;
        }

        if (playerFilter is not null)
        {
            args["player_filter"] = playerFilter;
        }

        if (providerFilter is not null)
        {
            args["provider_filter"] = providerFilter;
        }

        return await SendCommandAsync<AuthUser>("auth/user/update", args);
    }

    /// <summary>
    /// Logout (internal use)
    /// </summary>
    private void Logout()
    {
        ClearAuthSession();
        _httpClient.DefaultRequestHeaders.Authorization = null;
    }

    private void SetAuthHeader()
    {
        if (!IsAuthenticated || string.IsNullOrWhiteSpace(AuthToken))
        {
            _httpClient.DefaultRequestHeaders.Authorization = null;
            return;
        }

        var token = AuthToken;
        var current = _httpClient.DefaultRequestHeaders.Authorization;

        if (current?.Scheme == "Bearer" && current.Parameter == token)
        {
            return;
        }

        _httpClient.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", token);
    }

    public void SetAuthSession(string token, string? username = null)
    {
        if (!string.IsNullOrWhiteSpace(token))
        {
            _authToken = token;
            _isAuthenticated = true;
        }

        if (!string.IsNullOrWhiteSpace(username))
        {
            _settings.Username = username;
        }

        _settings.AuthToken = token;
        _settings.Save();

        _logger.LogInformation("Authentication session updated");
    }

    public void ClearAuthSession()
    {
        _authToken = null;
        _isAuthenticated = false;

        _settings.Username = null;
        _settings.AuthToken = null;
        _settings.Save();

        _logger.LogInformation("Logged out");
    }

    public void RequestLogin()
    {
        LoginRequired?.Invoke(this, EventArgs.Empty);
    }

    #endregion

    #region General

    /// <summary>
    /// Return Info of this server.
    /// </summary>
    public async Task<ServerInfoMessage?> GetServerInfoAsync()
    {
        return await SendCommandAsync<ServerInfoMessage>("info");
    }

    /// <summary>
    /// Return all loaded/running Providers (instances). Optionally filtered by ProviderType. Note that this applies user filters for music providers (for non admin users).
    /// </summary>
    public async Task<List<object>?> GetProvidersAsync()
    {
        return await SendCommandAsync<List<object>>("providers");
    }

    #endregion

    #region Music - Libraray

    /// <summary>
    /// Return a single item from the library by media type, item id and provider instance/domain.
    /// </summary>
    public async Task<MediaItem?> GetLibraryItemAsync(MediaType mediaType, string itemId, string providerInstanceIdOrDomain = "library")
    {
        return mediaType switch
        {
            MediaType.Artist => await GetLibraryItemAsync<Artist>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Album => await GetLibraryItemAsync<Album>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Track => await GetLibraryItemAsync<Track>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Playlist => await GetLibraryItemAsync<Playlist>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Radio => await GetLibraryItemAsync<Radio>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Audiobook => await GetLibraryItemAsync<Audiobook>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Podcast => await GetLibraryItemAsync<Podcast>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.PodcastEpisode => await GetLibraryItemAsync<PodcastEpisode>(mediaType, itemId, providerInstanceIdOrDomain),
            MediaType.Genre => await GetLibraryItemAsync<Genre>(mediaType, itemId, providerInstanceIdOrDomain),
            _ => null
        };
    }

    /// <summary>
    /// Return a single typed item from the library by media type, item id and provider instance/domain.
    /// </summary>
    public async Task<T?> GetLibraryItemAsync<T>(MediaType mediaType, string itemId, string providerInstanceIdOrDomain = "library")
    {
        _logger.LogInformation("Fetching library item: {ItemId} ({MediaType}) from {Provider}", itemId, mediaType, providerInstanceIdOrDomain);

        var args = new
        {
            media_type = mediaType.ToString().ToLowerInvariant(),
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var item = await SendCommandAsync<T>("music/get_library_item", args);
        if (item is Playlist playlist)
        {
            ResolveMediaItemImages(playlist);
            _ = EnrichWithProviderInfoAsync(new List<Playlist> { playlist });
        }

        return item;
    }

    #endregion

    #region Music - Artists

    /// <summary>
    /// Get in-database (album) artists.
    /// </summary>
    public async Task<List<Artist>> GetLibraryArtistsAsync(
        bool? favorite = null,
        string? search = null,
        int? limit = null,
        int? offset = null,
        string? orderBy = null,
        bool libraryItemsOnly = true)
    {
        _logger.LogInformation("Fetching library artists...");

        var args = new Dictionary<string, object>
        {
            ["library_items_only"] = libraryItemsOnly
        };

        if (favorite.HasValue) args["favorite"] = favorite.Value;
        if (!string.IsNullOrEmpty(search)) args["search"] = search;
        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;
        if (!string.IsNullOrEmpty(orderBy)) args["order_by"] = orderBy;

        var result = await SendCommandAsync<List<Artist>>("music/artists/library_items", args);
        var artists = result ?? new List<Artist>();

        _ = EnrichWithProviderInfoAsync(artists);

        return artists;
    }

    /// <summary>
    /// Return (full) details for a single media item.
    /// </summary>
    public async Task<Artist?> GetArtistAsync(string itemId, string providerInstanceIdOrDomain)
    {
        _logger.LogInformation("Fetching artist: {ItemId} from {Provider}", itemId, providerInstanceIdOrDomain);

        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var artist = await SendCommandAsync<Artist>("music/artists/get", args);
        if (artist != null)
        {
            _ = EnrichWithProviderInfoAsync(new List<Artist> { artist });
        }

        return artist;
    }

    /// <summary>
    /// Return (all/most popular) albums for an artist.
    /// </summary>
    public async Task<List<Album>> GetArtistAlbumsAsync(string itemId, string providerInstanceIdOrDomain, bool inLibraryOnly = false)
    {
        _logger.LogInformation("Fetching albums for artist: {ItemId}", itemId);

        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain,
            in_library_only = inLibraryOnly
        };

        var result = await SendCommandAsync<List<Album>>("music/artists/artist_albums", args);
        var albums = result ?? new List<Album>();

        _ = EnrichWithProviderInfoAsync(albums);
        return albums;
    }

    /// <summary>
    /// Return all/top tracks for an artist.
    /// </summary>
    public async Task<List<Track>> GetArtistTracksAsync(string itemId, string providerInstanceIdOrDomain, bool inLibraryOnly = false)
    {
        _logger.LogInformation("Fetching tracks for artist: {ItemId}", itemId);

        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["provider_instance_id_or_domain"] = providerInstanceIdOrDomain,
            ["in_library_only"] = inLibraryOnly
        };

        var result = await SendCommandAsync<List<Track>>("music/artists/artist_tracks", args);
        var tracks = result ?? new List<Track>();

        // Enrich with provider information
        _ = EnrichWithProviderInfoAsync(tracks);
        return result ?? new List<Track>();
    }

    /// <summary>
    /// Return top/featured tracks for an artist.
    /// For a library artist this can aggregate tracks across mapped providers,
    /// optionally restricted to a single provider instance.
    /// </summary>
    public async Task<List<Track>> GetArtistTopTracksAsync(
        string itemId,
        string providerInstanceIdOrDomain,
        string? providerFilter = null)
    {
        _logger.LogInformation(
            "Fetching top tracks for artist: {ItemId} (provider_filter: {ProviderFilter})",
            itemId,
            providerFilter ?? "<none>");

        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["provider_instance_id_or_domain"] = providerInstanceIdOrDomain
        };

        if (!string.IsNullOrWhiteSpace(providerFilter))
        {
            args["provider_filter"] = providerFilter;
        }

        var result = await SendCommandAsync<List<Track>>("music/artists/top_tracks", args);
        var tracks = result ?? new List<Track>();

        _ = EnrichWithProviderInfoAsync(tracks);
        return tracks;
    }

    /// <summary>
    /// Return top/featured albums for an artist.
    /// For a library artist this can aggregate albums across mapped providers,
    /// optionally restricted to a single provider instance.
    /// </summary>
    public async Task<List<Album>> GetArtistTopAlbumsAsync(
        string itemId,
        string providerInstanceIdOrDomain,
        string? providerFilter = null)
    {
        _logger.LogInformation(
            "Fetching top albums for artist: {ItemId} (provider_filter: {ProviderFilter})",
            itemId,
            providerFilter ?? "<none>");

        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["provider_instance_id_or_domain"] = providerInstanceIdOrDomain
        };

        if (!string.IsNullOrWhiteSpace(providerFilter))
        {
            args["provider_filter"] = providerFilter;
        }

        var result = await SendCommandAsync<List<Album>>("music/artists/top_albums", args);
        var albums = result ?? new List<Album>();

        _ = EnrichWithProviderInfoAsync(albums);
        return albums;
    }

    /// <summary>
    /// Return similar artists for an artist.
    /// For a library artist this can aggregate similar artists across mapped
    /// providers, optionally restricted to a single provider instance.
    /// </summary>
    public async Task<List<Artist>> GetSimilarArtistsAsync(
        string itemId,
        string providerInstanceIdOrDomain,
        string? providerFilter = null,
        int? limit = null)
    {
        _logger.LogInformation(
            "Fetching similar artists for artist: {ItemId} (provider_filter: {ProviderFilter}, limit: {Limit})",
            itemId,
            providerFilter ?? "<none>",
            limit?.ToString() ?? "<none>");

        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["provider_instance_id_or_domain"] = providerInstanceIdOrDomain
        };

        if (!string.IsNullOrWhiteSpace(providerFilter))
        {
            args["provider_filter"] = providerFilter;
        }

        if (limit.HasValue)
        {
            args["limit"] = limit.Value;
        }

        var result = await SendCommandAsync<List<Artist>>("music/artists/similar_artists", args);
        var artists = result ?? new List<Artist>();

        _ = EnrichWithProviderInfoAsync(artists);
        return artists;
    }

    /// <summary>
    /// Return the total number of items in the library.
    /// </summary>
    public async Task<int> GetArtistsCountAsync(bool favoriteOnly = false)
    {
        var args = new { favorite_only = favoriteOnly };
        return await SendCommandAsync<int>("music/artists/count", args);
    }

    #endregion

    #region Music - Albums

    /// <summary>
    /// Get in-database albums.
    /// </summary>
    public async Task<List<Album>> GetLibraryAlbumsAsync(
        bool? favorite = null,
        string? search = null,
        int? limit = null,
        int? offset = null,
        string? orderBy = null,
        bool libraryItemsOnly = true,
        AlbumType[]? albumTypes = null)
    {
        _logger.LogInformation("Fetching library albums...");

        var args = new Dictionary<string, object>
        {
            ["library_items_only"] = libraryItemsOnly
        };

        if (favorite.HasValue) args["favorite"] = favorite.Value;
        if (!string.IsNullOrEmpty(search)) args["search"] = search;
        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;
        if (!string.IsNullOrEmpty(orderBy)) args["order_by"] = orderBy;
        if (albumTypes != null && albumTypes.Length > 0)
        {
            args["album_types"] = albumTypes.Select(t => t.ToString().ToLowerInvariant()).ToList();
        }

        var result = await SendCommandAsync<List<Album>>("music/albums/library_items", args);
        var albums = result ?? new List<Album>();

        _ = EnrichWithProviderInfoAsync(albums);

        return albums;
    }

    /// <summary>
    /// Return (full) details for a single media item.
    /// </summary>
    public async Task<Album?> GetAlbumAsync(string itemId, string providerInstanceIdOrDomain)
    {
        _logger.LogInformation("Fetching album: {ItemId} from {Provider}", itemId, providerInstanceIdOrDomain);

        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var album = await SendCommandAsync<Album>("music/albums/get", args);
        if (album != null)
        {
            _ = EnrichWithProviderInfoAsync(new List<Album> { album });
        }

        return album;
    }

    /// <summary>
    /// Return album tracks for the given provider album id.
    /// </summary>
    public async Task<List<Track>> GetAlbumTracksAsync(string itemId, string providerInstanceIdOrDomain, bool inLibraryOnly = false)
    {
        _logger.LogInformation("Fetching tracks for album: {ItemId}", itemId);

        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain,
            in_library_only = inLibraryOnly
        };

        var result = await SendCommandAsync<List<Track>>("music/albums/album_tracks", args);
        var tracks = result ?? new List<Track>();

        _ = EnrichWithProviderInfoAsync(tracks);
        return tracks;
    }

    /// <summary>
    /// Return all versions of an album we can find on all providers.
    /// </summary>
    public async Task<List<Album>> GetAlbumVersionsAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var result = await SendCommandAsync<List<Album>>("music/albums/album_versions", args);
        return result ?? new List<Album>();
    }

    /// <summary>
    /// Return the total number of items in the library.
    /// </summary>
    public async Task<int> GetAlbumsCountAsync(bool favoriteOnly = false, AlbumType[]? albumTypes = null)
    {
        var args = new Dictionary<string, object>
        {
            ["favorite_only"] = favoriteOnly
        };

        if (albumTypes != null && albumTypes.Length > 0)
        {
            args["album_types"] = albumTypes.Select(t => t.ToString().ToLowerInvariant()).ToList();
        }

        return await SendCommandAsync<int>("music/albums/count", args);
    }

    #endregion

    #region Music - Tracks

    /// <summary>
    /// Get in-database tracks.
    /// </summary>
    public async Task<List<Track>> GetLibraryTracksAsync(
        bool? favorite = null,
        string? search = null,
        int? limit = null,
        int? offset = null,
        string? orderBy = null,
        bool libraryItemsOnly = true)
    {
        _logger.LogInformation("Fetching library tracks...");

        var args = new Dictionary<string, object>
        {
            ["library_items_only"] = libraryItemsOnly
        };

        if (favorite.HasValue) args["favorite"] = favorite.Value;
        if (!string.IsNullOrEmpty(search)) args["search"] = search;
        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;
        if (!string.IsNullOrEmpty(orderBy)) args["order_by"] = orderBy;

        var result = await SendCommandAsync<List<Track>>("music/tracks/library_items", args);
        var tracks = result ?? new List<Track>();

        _ = EnrichWithProviderInfoAsync(tracks);

        return tracks;
    }

    /// <summary>
    /// Return (full) details for a single media item.
    /// </summary>
    public async Task<Track?> GetTrackAsync(string itemId, string providerInstanceIdOrDomain)
    {
        _logger.LogInformation("Fetching track: {ItemId} from {Provider}", itemId, providerInstanceIdOrDomain);

        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var track = await SendCommandAsync<Track>("music/tracks/get", args);
        if (track != null)
        {
            _ = EnrichWithProviderInfoAsync(new List<Track> { track });
        }

        return track;
    }

    /// <summary>
    /// Return all versions of a track we can find on all providers.
    /// </summary>
    public async Task<List<Track>> GetTrackVersionsAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var result = await SendCommandAsync<List<Track>>("music/tracks/track_versions", args);
        var tracks = result ?? new List<Track>();

        _ = EnrichWithProviderInfoAsync(tracks);

        return tracks;
    }

    /// <summary>
    /// Return all albums the track appears on.
    /// </summary>
    public async Task<List<Album>> GetTrackAlbumsAsync(string itemId, string providerInstanceIdOrDomain, bool inLibraryOnly = false)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain,
            in_library_only = inLibraryOnly
        };

        var result = await SendCommandAsync<List<Album>>("music/tracks/track_albums", args);
        var albums = result ?? new List<Album>();

        _ = EnrichWithProviderInfoAsync(albums);

        return albums;
    }

    /// <summary>
    /// Return the total number of items in the library.
    /// </summary>
    public async Task<int> GetTracksCountAsync(bool favoriteOnly = false)
    {
        var args = new { favorite_only = favoriteOnly };
        return await SendCommandAsync<int>("music/tracks/count", args);
    }

    /// <summary>
    /// Return url to short preview sample.
    /// </summary>
    public async Task<string?> GetTrackPreviewAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        return await SendCommandAsync<string>("music/tracks/preview", args);
    }

    /// <summary>
    /// Return similar tracks for a given track.
    /// </summary>
    public async Task<List<Track>> GetSimilarTracksAsync(
        string itemId,
        string providerInstanceIdOrDomain,
        int? limit = null,
        bool? allowLookup = null,
        List<string>? preferredProviderInstances = null)
    {
        _logger.LogInformation("Fetching similar tracks for: {ItemId}", itemId);

        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["provider_instance_id_or_domain"] = providerInstanceIdOrDomain
        };

        if (limit.HasValue) args["limit"] = limit.Value;
        if (allowLookup.HasValue) args["allow_lookup"] = allowLookup.Value;
        if (preferredProviderInstances != null && preferredProviderInstances.Count > 0)
        {
            args["preferred_provider_instances"] = preferredProviderInstances;
        }

        var result = await SendCommandAsync<List<Track>>("music/tracks/similar_tracks", args);
        var tracks = result ?? new List<Track>();

        _ = EnrichWithProviderInfoAsync(tracks);

        return tracks;
    }

    #endregion

    #region Music - Playlists

    /// <summary>
    /// Get in-database playlists.
    /// </summary>
    public async Task<List<Playlist>> GetLibraryPlaylistsAsync(
        bool? favorite = null,
        string? search = null,
        int? limit = null,
        int? offset = null,
        string? orderBy = null,
        bool libraryItemsOnly = true,
        string? userPrefix = null)
    {
        _logger.LogInformation("Fetching library playlists...");

        var normalizedUserPrefix = string.IsNullOrWhiteSpace(userPrefix)
            ? null
            : userPrefix.Trim();

        
        if (!string.IsNullOrWhiteSpace(normalizedUserPrefix)
            && string.IsNullOrWhiteSpace(search))
        {
            // Hint backend search to reduce payload; local filtering below remains authoritative.
            search = normalizedUserPrefix;
        }

        var args = new Dictionary<string, object>
        {
            ["library_items_only"] = libraryItemsOnly
        };

        if (favorite.HasValue) args["favorite"] = favorite.Value;
        if (!string.IsNullOrEmpty(search)) args["search"] = search;
        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;
        if (!string.IsNullOrEmpty(orderBy)) args["order_by"] = orderBy;

        var result = await SendCommandAsync<List<Playlist>>("music/playlists/library_items", args);
        var playlists = result ?? new List<Playlist>();

        foreach (var playlist in playlists)
        {
            ResolveMediaItemImages(playlist);
        }

        if (!string.IsNullOrWhiteSpace(normalizedUserPrefix))
        {
            playlists = playlists
                .Where(playlist => !string.IsNullOrWhiteSpace(playlist.Name)
                    && playlist.Name.StartsWith(normalizedUserPrefix, StringComparison.OrdinalIgnoreCase))
                .ToList();

            foreach (var playlist in playlists)
            {
                playlist.DisplayName = playlist.Name[normalizedUserPrefix.Length..];
            }
        }

        _ = EnrichWithProviderInfoAsync(playlists);

        return playlists;
    }

    /// <summary>
    /// Return (full) details for a single media item.
    /// </summary>
    public async Task<Playlist?> GetPlaylistAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var playlist = await SendCommandAsync<Playlist>("music/playlists/get", args);
        if (playlist != null)
        {
            ResolveMediaItemImages(playlist);
            _ = EnrichWithProviderInfoAsync(new List<Playlist> { playlist });
        }

        return playlist;
    }

    /// <summary>
    /// Return playlist tracks for the given provider playlist id.
    /// </summary>
    public async Task<List<Track>> GetPlaylistTracksAsync(string itemId, string providerInstanceIdOrDomain, bool forceRefresh = false)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain,
            force_refresh = forceRefresh
        };

        var result = await SendCommandAsync<List<Track>>("music/playlists/playlist_tracks", args);
        var tracks = result ?? new List<Track>();

        _ = EnrichWithProviderInfoAsync(tracks);
        return result ?? new List<Track>();
    }

    /// <summary>
    /// Create new playlist.
    /// </summary>
    public async Task<Playlist?> CreatePlaylistAsync(string name, string? providerInstanceOrDomain = null)
    {
        var args = new Dictionary<string, object>
        {
            ["name"] = name
        };

        if (!string.IsNullOrEmpty(providerInstanceOrDomain))
        {
            args["provider_instance_or_domain"] = providerInstanceOrDomain;
        }

        return await SendCommandAsync<Playlist>("music/playlists/create_playlist", args);
    }

    /// <summary>
    /// Add tracks to playlist.
    /// </summary>
    public async Task AddPlaylistTracksAsync(string playlistId, List<string> uris)
    {
        var args = new
        {
            db_playlist_id = playlistId,
            uris = uris
        };

        await SendCommandAsync<object>("music/playlists/add_playlist_tracks", args);
    }

    /// <summary>
    /// Remove multiple tracks from playlist.
    /// </summary>
    public async Task RemovePlaylistTracksAsync(string playlistId, List<int> positionsToRemove)
    {
        var args = new
        {
            db_playlist_id = playlistId,
            positions_to_remove = positionsToRemove
        };

        await SendCommandAsync<object>("music/playlists/remove_playlist_tracks", args);
    }

    /// <summary>
    /// Remove a playlist from the library.
    /// </summary>
    public async Task RemovePlaylistAsync(string itemId, bool recursive = false)
    {
        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["recursive"] = recursive
        };

        await SendCommandAsync<object>("music/playlists/remove", args);
    }

    /// <summary>
    /// Update an existing playlist in the library.
    /// </summary>
    public async Task<Playlist?> UpdatePlaylistAsync(string itemId, Playlist update, bool overwrite = false)
    {
        var updateImage = update.Metadata?.Images?.FirstOrDefault();
        if (updateImage != null)
        {
            updateImage.Path = RestoreImagePath(updateImage.Path, _settings.MusicAssistantUrl);
        }

        var updatePayload = new Dictionary<string, object?>
        {
            ["item_id"] = update.ItemId,
            ["provider"] = update.Provider,
            ["name"] = update.Name,
            ["sort_name"] = update.SortName,
            ["uri"] = update.Uri,
            ["media_type"] = "playlist",
            ["provider_mappings"] = update.ProviderMappings,
            ["metadata"] = update.Metadata,
            ["favorite"] = update.Favorite,
            ["external_ids"] = update.ExternalIds,
            ["owner"] = update.Owner,
            ["is_editable"] = update.IsEditable
        };

        var args = new Dictionary<string, object>
        {
            ["item_id"] = itemId,
            ["update"] = updatePayload,
            ["overwrite"] = overwrite
        };

        return await SendCommandAsync<Playlist>("music/playlists/update", args);
    }

    /// <summary>
    /// Return the total number of items in the library.
    /// </summary>
    public async Task<int> GetPlaylistsCountAsync(bool favoriteOnly = false)
    {
        var args = new { favorite_only = favoriteOnly };
        return await SendCommandAsync<int>("music/playlists/count", args);
    }

    #endregion

    #region Music - Radio

    /// <summary>
    /// Get in-database radio stations.
    /// </summary>
    public async Task<List<Radio>> GetLibraryRadiosAsync(
        bool? favorite = null,
        string? search = null,
        int? limit = null,
        int? offset = null,
        string? orderBy = null,
        bool libraryItemsOnly = true)
    {
        var args = new Dictionary<string, object>
        {
            ["library_items_only"] = libraryItemsOnly
        };

        if (favorite.HasValue) args["favorite"] = favorite.Value;
        if (!string.IsNullOrEmpty(search)) args["search"] = search;
        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;
        if (!string.IsNullOrEmpty(orderBy)) args["order_by"] = orderBy;

        var result = await SendCommandAsync<List<Radio>>("music/radios/library_items", args);
        return result ?? new List<Radio>();
    }

    /// <summary>
    /// Return (full) details for a single media item.
    /// </summary>
    public async Task<Radio?> GetRadioAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        return await SendCommandAsync<Radio>("music/radios/get", args);
    }

    /// <summary>
    /// Return the total number of items in the library.
    /// </summary>
    public async Task<int> GetRadiosCountAsync(bool favoriteOnly = false)
    {
        var args = new { favorite_only = favoriteOnly };
        return await SendCommandAsync<int>("music/radios/count", args);
    }

    #endregion

    #region Music - Podcasts

    /// <summary>
    /// Get in-database podcasts.
    /// </summary>
    public async Task<List<Podcast>> GetLibraryPodcastsAsync(
        bool? favorite = null,
        string? search = null,
        int? limit = null,
        int? offset = null,
        string? orderBy = null,
        bool libraryItemsOnly = true)
    {
        var args = new Dictionary<string, object>
        {
            ["library_items_only"] = libraryItemsOnly
        };

        if (favorite.HasValue) args["favorite"] = favorite.Value;
        if (!string.IsNullOrEmpty(search)) args["search"] = search;
        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;
        if (!string.IsNullOrEmpty(orderBy)) args["order_by"] = orderBy;

        var result = await SendCommandAsync<List<Podcast>>("music/podcasts/library_items", args);
        return result ?? new List<Podcast>();
    }

    /// <summary>
    /// Return (full) details for a single media item.
    /// </summary>
    public async Task<Podcast?> GetPodcastAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        return await SendCommandAsync<Podcast>("music/podcasts/get", args);
    }

    /// <summary>
    /// Return podcast episodes for the given provider podcast id.
    /// </summary>
    public async Task<List<PodcastEpisode>> GetPodcastEpisodesAsync(string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        var result = await SendCommandAsync<List<PodcastEpisode>>("music/podcasts/podcast_episodes", args);
        return result ?? new List<PodcastEpisode>();
    }

    /// <summary>
    /// Return the total number of items in the library.
    /// </summary>
    public async Task<int> GetPodcastsCountAsync(bool favoriteOnly = false)
    {
        var args = new { favorite_only = favoriteOnly };
        return await SendCommandAsync<int>("music/podcasts/count", args);
    }

    #endregion

    #region Music - Search & Browse

    /// <summary>
    /// Perform global search for media items on all providers.
    /// </summary>
    public async Task<SearchResults?> SearchAsync(
        string searchQuery,
        MediaType[]? mediaTypes = null,
        int? limit = null,
        bool libraryOnly = false)
    {
        _logger.LogInformation("Searching for: {Query}", searchQuery);

        var args = new Dictionary<string, object>
        {
            ["search_query"] = searchQuery,
            ["library_only"] = libraryOnly
        };

        if (mediaTypes != null && mediaTypes.Length > 0)
        {
            args["media_types"] = mediaTypes.Select(mt => mt.ToString().ToLowerInvariant()).ToList();
        }
        if (limit.HasValue) args["limit"] = limit.Value;

        var results = await SendCommandAsync<SearchResults>("music/search", args);

        if (results != null)
        {
            ResolveMediaItemImages(results.Playlists ?? new List<Playlist>());

            _ = EnrichWithProviderInfoAsync(results.Tracks ?? new List<Track>());
            _ = EnrichWithProviderInfoAsync(results.Albums ?? new List<Album>());
            _ = EnrichWithProviderInfoAsync(results.Artists ?? new List<Artist>());
            _ = EnrichWithProviderInfoAsync(results.Playlists ?? new List<Playlist>());
        }

        return results;
    }

    /// <summary>
    /// Browse Music providers.
    /// </summary>
    public async Task<List<object>?> BrowseAsync(string? path = null)
    {
        var args = string.IsNullOrEmpty(path) ? null : new { path = path };
        return await SendCommandAsync<List<object>>("music/browse", args);
    }

    /// <summary>
    /// Return recommendation folders.
    /// </summary>
    public async Task<List<RecommendationFolder>> GetRecommendationsAsync()
    {
        var result = await SendCommandAsync<List<RecommendationFolder>>("music/recommendations");
        var folders = result ?? new List<RecommendationFolder>();

        foreach (var folder in folders)
        {
            if (folder.Image != null)
            {
                folder.Image.Path = ResolveImagePath(folder.Image.Path, folder.Image.Provider);
            }

            if (folder.Items != null && folder.Items.Count > 0)
            {
                ResolveMediaItemImages(folder.Items);
            }
        }

        var items = folders
            .SelectMany(folder => folder.Items ?? Enumerable.Empty<MediaItem>())
            .Where(item => item != null)
            .ToList();

        if (items.Count > 0)
        {
            _ = EnrichWithProviderInfoAsync(items);
        }

        return folders;
    }

    /// <summary>
    /// Get single music item by id and media type.
    /// </summary>
    public async Task<object?> GetMusicItemAsync(MediaType mediaType, string itemId, string providerInstanceIdOrDomain)
    {
        var args = new
        {
            media_type = mediaType.ToString().ToLowerInvariant(),
            item_id = itemId,
            provider_instance_id_or_domain = providerInstanceIdOrDomain
        };

        return await SendCommandAsync<object>("music/item", args);
    }

    /// <summary>
    /// Fetch MediaItem by uri.
    /// </summary>
    public async Task<object?> GetItemByUriAsync(string uri)
    {
        var args = new { uri = uri };
        return await SendCommandAsync<object>("music/item_by_uri", args);
    }

    /// <summary>
    /// Fetch MediaItem by uri and deserialize to its concrete type.
    /// </summary>
    public async Task<MediaItem?> ResolveItemByUriAsync(string uri)
    {
        if (string.IsNullOrWhiteSpace(uri))
        {
            return null;
        }

        try
        {
            var args = new { uri = uri };
            var element = await SendCommandAsync<JsonElement>("music/item_by_uri", args);

            if (element.ValueKind != JsonValueKind.Object)
            {
                return null;
            }

            if (!element.TryGetProperty("media_type", out var mediaTypeValue))
            {
                return null;
            }

            var mediaType = mediaTypeValue.GetString()?.ToLowerInvariant();
            return mediaType switch
            {
                "track" => await DeserializeMediaItemAsync<Track>(element),
                "album" => await DeserializeMediaItemAsync<Album>(element),
                "playlist" => await DeserializeMediaItemAsync<Playlist>(element),
                "artist" => await DeserializeMediaItemAsync<Artist>(element),
                _ => null
            };
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to resolve media item by uri: {Uri}", uri);
            return null;
        }
    }

    /// <summary>
    /// Return a list of the last added tracks.
    /// </summary>
    public async Task<List<Track>> GetRecentlyAddedTracksAsync(int limit = 50)
    {
        var args = new { limit = limit };
        var result = await SendCommandAsync<List<Track>>("music/recently_added_tracks", args);
        return result ?? new List<Track>();
    }

    /// <summary>
    /// Return a list of the last played items. Returns various media types.
    /// </summary>
    public async Task<List<MediaItem>> GetRecentlyPlayedItemsAsync(
        int limit = 50,
        MediaType[]? mediaTypes = null,
        string? userId = null,
        string? queueId = null,
        bool? fullyPlayedOnly = null,
        bool? userInitiatedOnly = null)
    {
        var args = new Dictionary<string, object>
        {
            ["limit"] = limit
        };

        if (mediaTypes != null && mediaTypes.Length > 0)
        {
            args["media_types"] = mediaTypes.Select(mt => mt.ToString().ToLowerInvariant()).ToList();
        }

        if (!string.IsNullOrWhiteSpace(userId))
        {
            args["user_id"] = userId;
        }

        if (!string.IsNullOrWhiteSpace(queueId))
        {
            args["queue_id"] = queueId;
        }

        if (fullyPlayedOnly.HasValue)
        {
            args["fully_played_only"] = fullyPlayedOnly.Value;
        }

        if (userInitiatedOnly.HasValue)
        {
            args["user_initiated_only"] = userInitiatedOnly.Value;
        }

        var result = await SendCommandAsync<List<MediaItem>>("music/recently_played_items", args);
        var items = result ?? new List<MediaItem>();

        var tracks = items.OfType<Track>().ToList();
        var albums = items.OfType<Album>().ToList();
        var artists = items.OfType<Artist>().ToList();
        var playlists = items.OfType<Playlist>().ToList();

        ResolveMediaItemImages(playlists);

        _ = EnrichWithProviderInfoAsync(tracks);
        _ = EnrichWithProviderInfoAsync(albums);
        _ = EnrichWithProviderInfoAsync(artists);
        _ = EnrichWithProviderInfoAsync(playlists);

        return items;
    }

    #endregion

    #region Music - Library Management

    /// <summary>
    /// Add item by given uri to the library.
    /// </summary>
    public async Task<object?> AddLibraryItemAsync(string uri, bool overwriteExisting = false)
    {
        return await AddLibraryItemAsync((object)uri, overwriteExisting);
    }

    /// <summary>
    /// Add item by given MediaItem object to the library.
    /// </summary>
    public async Task<object?> AddLibraryItemAsync(MediaItem mediaItem, bool overwriteExisting = false)
    {
        var itemPayload = new
        {
            item_id = mediaItem.ItemId,
            provider = mediaItem.Provider,
            name = mediaItem.Name,
            sort_name = mediaItem.SortName,
            uri = mediaItem.Uri,
            media_type = mediaItem.MediaType == MediaType.PodcastEpisode
                ? "podcast_episode"
                : mediaItem.MediaType.ToString().ToLowerInvariant(),
            provider_mappings = mediaItem.ProviderMappings.Select(mapping => new
            {
                item_id = mapping.ItemId,
                provider_domain = mapping.ProviderDomain,
                provider_instance = mapping.ProviderInstance,
                available = mapping.Available,
                url = mapping.Url
            }).ToList()
        };

        return await AddLibraryItemAsync((object)itemPayload, overwriteExisting);
    }

    /// <summary>
    /// Add item (uri or mediaitem) to the library.
    /// </summary>
    public async Task<object?> AddLibraryItemAsync(object item, bool overwriteExisting = false)
    {
        var args = new
        {
            item,
            overwrite_existing = overwriteExisting
        };

        return await SendCommandAsync<object>("music/library/add_item", args);
    }

    /// <summary>
    /// Remove item from the library. Destructive! Will remove the item and all dependants.
    /// </summary>
    public async Task RemoveLibraryItemAsync(MediaType mediaType, string libraryItemId, bool recursive = false)
    {
        var args = new
        {
            media_type = mediaType.ToString().ToLowerInvariant(),
            library_item_id = libraryItemId,
            recursive = recursive
        };

        await SendCommandAsync<object>("music/library/remove_item", args);
    }

    /// <summary>
    /// Add an item to the favorites.
    /// </summary>
    public async Task AddFavoriteAsync(string itemUri)
    {
        var args = new { item = itemUri };
        await SendCommandAsync<object>("music/favorites/add_item", args);
    }

    /// <summary>
    /// Remove (library) item from the favorites.
    /// </summary>
    public async Task RemoveFavoriteAsync(MediaType mediaType, string libraryItemId)
    {
        var args = new
        {
            media_type = mediaType.ToString().ToLowerInvariant(),
            library_item_id = libraryItemId
        };

        await SendCommandAsync<object>("music/favorites/remove_item", args);
    }

    #endregion

    #region Player Queues

    /// <summary>
    /// Return all registered PlayerQueues.
    /// </summary>
    public async Task<List<PlayerQueue>> GetPlayerQueuesAsync()
    {
        _logger.LogInformation("Fetching player queues...");

        var result = await SendCommandAsync<List<PlayerQueue>>("player_queues/all");
        return result ?? new List<PlayerQueue>();
    }

    /// <summary>
    /// Return PlayerQueue by queue_id or None if not found.
    /// </summary>
    public async Task<PlayerQueue?> GetPlayerQueueAsync(string queueId)
    {
        var args = new { queue_id = queueId };
        return await SendCommandAsync<PlayerQueue>("player_queues/get", args);
    }

    /// <summary>
    /// Return the current active/synced queue for a player.
    /// </summary>
    public async Task<PlayerQueue?> GetActiveQueueForPlayerAsync(string playerId)
    {
        _logger.LogDebug("Fetching active queue for player: {PlayerId}", playerId);

        var args = new { player_id = playerId };
        var queue = await SendCommandAsync<PlayerQueue>("player_queues/get_active_queue", args);

        var currentTrack = queue?.CurrentItem?.MediaItem;
        if (currentTrack != null)
        {
            _ = EnrichWithProviderInfoAsync(new List<Track> { currentTrack });
        }

        return queue;
    }

    /// <summary>
    /// Return all QueueItems for given PlayerQueue.
    /// </summary>
    public async Task<List<QueueItem>> GetQueueItemsAsync(
        string queueId,
        int? limit = null,
        int? offset = null,
        bool useSortIndexRankForDisplay = false)
    {
        _logger.LogInformation("Fetching queue items for: {QueueId}", queueId);

        var args = new Dictionary<string, object>
        {
            ["queue_id"] = queueId
        };

        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;

        var result = await SendCommandAsync<List<QueueItem>>("player_queues/items", args);
        var queueItems = result ?? new List<QueueItem>();

        Dictionary<int, int>? sortIndexRankMap = null;
        if (useSortIndexRankForDisplay)
        {
            sortIndexRankMap = queueItems
                .Where(item => item.SortIndex.HasValue)
                .Select(item => item.SortIndex!.Value)
                .Distinct()
                .OrderBy(sortIndex => sortIndex)
                .Select((sortIndex, rank) => new { sortIndex, rank })
                .ToDictionary(entry => entry.sortIndex, entry => entry.rank);
        }

        for (var i = 0; i < queueItems.Count; i++)
        {
            var queueItem = queueItems[i];

            if (useSortIndexRankForDisplay
                && queueItem.SortIndex.HasValue
                && sortIndexRankMap != null
                && sortIndexRankMap.TryGetValue(queueItem.SortIndex.Value, out var rank))
            {
                queueItem.Index = rank;
            }
            else
            {
                queueItem.Index = i;
            }

            // Keep Track.Index positional for command flows that target play_index.
            if (queueItem.MediaItem is Track track)
            {
                track.Index = i + 1;
            }
        }

        var queueTracks = queueItems
            .Select(queueItem => queueItem.MediaItem)
            .OfType<Track>()
            .ToList();

        if (queueTracks.Count > 0)
        {
            _ = EnrichWithProviderInfoAsync(queueTracks);
        }

        return queueItems;
    }

    /// <summary>
    /// Play media items on the given queue.
    /// </summary>
    public async Task PlayMediaAsync(
     string queueId,
     List<MediaItem> mediaItems,
     QueueOption option = QueueOption.Replace,
        bool radioMode = false,
        object? startItem = null,
        string? sortBy = "original")
    {
        if (mediaItems == null || mediaItems.Count == 0)
        {
            throw new ArgumentException("Media items list cannot be null or empty", nameof(mediaItems));
        }

        var mediaPayload = mediaItems
            .Where(item => !string.IsNullOrWhiteSpace(item.ItemId) && !string.IsNullOrWhiteSpace(item.Provider))
            .Select(CreateMediaItemPayload)
            .ToList();

        if (mediaPayload.Count == 0)
        {
            throw new ArgumentException("No valid media items with item_id/provider found", nameof(mediaItems));
        }

        _logger.LogInformation("Playing {Count} media item object(s) on queue: {QueueId}", mediaPayload.Count, queueId);

        var args = new Dictionary<string, object>
        {
            ["queue_id"] = queueId,
            ["media"] = mediaPayload,
            ["option"] = option.ToString().ToLowerInvariant(),
            ["radio_mode"] = radioMode
        };

        args["sort_by"] = string.IsNullOrWhiteSpace(sortBy) ? "original" : sortBy;

        MediaItem? resolvedStartItem = null;

        if (startItem is MediaItem startMediaItem)
        {
            resolvedStartItem = startMediaItem;
        }
        else if (startItem is string startItemString && !string.IsNullOrWhiteSpace(startItemString))
        {
            resolvedStartItem = mediaItems.FirstOrDefault(item =>
                string.Equals(item.Uri, startItemString, StringComparison.OrdinalIgnoreCase) ||
                string.Equals(item.ItemId, startItemString, StringComparison.OrdinalIgnoreCase));
        }

        // Default to first item when startItem is not provided or cannot be resolved.
        resolvedStartItem ??= mediaItems[0];
        args["start_item"] = CreateMediaItemPayload(resolvedStartItem);

        await SendCommandAsync<object>("player_queues/play_media", args);
    }

    // Helper to create the media item payload for play_media command, which requires a specific structure and may differ from the standard MediaItem serialization.
    private static object CreateMediaItemPayload(MediaItem mediaItem)
    {
        var payload = new Dictionary<string, object?>
        {
            ["item_id"] = mediaItem.ItemId,
            ["provider"] = mediaItem.Provider,
            ["name"] = string.IsNullOrWhiteSpace(mediaItem.Name) ? mediaItem.ItemId : mediaItem.Name,
            ["sort_name"] = mediaItem.SortName,
            ["uri"] = mediaItem.Uri,
            ["media_type"] = mediaItem.MediaType == MediaType.PodcastEpisode
                ? "podcast_episode"
                : mediaItem.MediaType.ToString().ToLowerInvariant(),
            ["provider_mappings"] = mediaItem.ProviderMappings.Select(mapping => new
            {
                item_id = mapping.ItemId,
                provider_domain = mapping.ProviderDomain,
                provider_instance = mapping.ProviderInstance,
                available = mapping.Available,
                url = mapping.Url
            }).ToList(),
            ["metadata"] = CreateMediaItemMetadataPayload(mediaItem.Metadata),
            ["favorite"] = mediaItem.Favorite
        };

        switch (mediaItem)
        {
            case Track track:
                payload["duration"] = track.Duration;
                payload["disc_number"] = track.DiscNumber;
                payload["track_number"] = track.TrackNumber;
                payload["artists"] = track.Artists?.Select(CreateMediaItemMappingPayload).ToList();
                payload["album"] = track.Album != null ? CreateMediaItemMappingPayload(track.Album) : null;
                break;
            case Album album:
                payload["year"] = album.Year;
                payload["album_type"] = album.AlbumType;
                payload["artists"] = album.Artists?.Select(CreateMediaItemMappingPayload).ToList();
                break;
            case Playlist playlist:
                payload["owner"] = playlist.Owner;
                payload["is_editable"] = playlist.IsEditable;
                break;
            case PodcastEpisode episode:
                payload["duration"] = episode.Duration;
                payload["podcast"] = episode.Podcast != null
                    ? CreateMediaItemMappingPayload(episode.Podcast)
                    : null;
                break;
        }

        return payload;
    }

    // Helper to create a simplified media item mapping payload for nested items (e.g. track's album or artists), which only includes basic info and is used in the play_media command.
    private static object CreateMediaItemMappingPayload(MediaItem mediaItem)
        => new
        {
            item_id = mediaItem.ItemId,
            provider = mediaItem.Provider,
            name = string.IsNullOrWhiteSpace(mediaItem.Name) ? mediaItem.ItemId : mediaItem.Name,
            version = (string?)null,
            sort_name = mediaItem.SortName,
            uri = mediaItem.Uri,
            available = true,
            metadata = CreateMediaItemMetadataPayload(mediaItem.Metadata),
            media_type = mediaItem.MediaType == MediaType.PodcastEpisode
                ? "podcast_episode"
                : mediaItem.MediaType.ToString().ToLowerInvariant()
        };

    private static object? CreateMediaItemMetadataPayload(MediaItemMetadata? metadata)
    {
        if (metadata == null)
        {
            return null;
        }

        return new
        {
            description = metadata.Description,
            images = metadata.Images?.Select(image => new
            {
                type = image.Type,
                path = image.Path,
                provider = image.Provider,
                remotely_accessible = image.RemotelyAccessible
            }).ToList(),
            genres = metadata.Genres,
            label = metadata.Label,
            popularity = metadata.Popularity,
            release_date = metadata.ReleaseDate
        };
    }

    /// <summary>
    /// Play a single media item on the given queue.
    /// </summary>
    public async Task PlayMediaAsync(
        string queueId,
        MediaItem mediaItem,
        QueueOption option = QueueOption.Play,
        bool radioMode = false,
        object? startItem = null,
        string? sortBy = "original")
    {
        await PlayMediaAsync(queueId, new List<MediaItem> { mediaItem }, option, radioMode, startItem, sortBy);
    }

    /// <summary>
    /// Handle PLAY command for given queue.
    /// </summary>
    public async Task PlayAsync(string queueId)
    {
        _logger.LogInformation("Play on queue: {QueueId}", queueId);
        await SendCommandAsync<object>("player_queues/play", new { queue_id = queueId });
    }

    /// <summary>
    /// Handle PAUSE command for given queue.
    /// </summary>
    public async Task PauseAsync(string queueId)
    {
        _logger.LogInformation("Pause on queue: {QueueId}", queueId);
        await SendCommandAsync<object>("player_queues/pause", new { queue_id = queueId });
    }

    /// <summary>
    /// Toggle play/pause on given playerqueue.
    /// </summary>
    public async Task PlayPauseAsync(string queueId)
    {
        _logger.LogInformation("Play/Pause on queue: {QueueId}", queueId);
        await SendCommandAsync<object>("player_queues/play_pause", new { queue_id = queueId });
    }

    /// <summary>
    /// Handle STOP command for given queue.
    /// </summary>
    public async Task StopAsync(string queueId)
    {
        _logger.LogInformation("Stop on queue: {QueueId}", queueId);
        await SendCommandAsync<object>("player_queues/stop", new { queue_id = queueId });
    }

    /// <summary>
    /// Handle NEXT TRACK command for given queue.
    /// </summary>
    public async Task NextAsync(string queueId)
    {
        _logger.LogInformation("Next on queue: {QueueId}", queueId);
        await SendCommandAsync<object>("player_queues/next", new { queue_id = queueId });
    }

    /// <summary>
    /// Handle PREVIOUS TRACK command for given queue.
    /// </summary>
    public async Task PreviousAsync(string queueId)
    {
        _logger.LogInformation("Previous on queue: {QueueId}", queueId);
        await SendCommandAsync<object>("player_queues/previous", new { queue_id = queueId });
    }

    /// <summary>
    /// Handle SEEK command for given queue.
    /// </summary>
    public async Task SeekAsync(string queueId, int position)
    {
        _logger.LogInformation("Seek on queue: {QueueId} to {Position}", queueId, position);
        await SendCommandAsync<object>("player_queues/seek", new { queue_id = queueId, position = position });
    }

    /// <summary>
    /// Handle SKIP command for given queue.
    /// </summary>
    public async Task SkipAsync(string queueId, int seconds)
    {
        await SendCommandAsync<object>("player_queues/skip", new { queue_id = queueId, seconds = seconds });
    }

    /// <summary>
    /// Clear all items in the queue.
    /// </summary>
    public async Task ClearQueueAsync(string queueId, bool skipStop = false)
    {
        await SendCommandAsync<object>("player_queues/clear", new { queue_id = queueId, skip_stop = skipStop });
    }

    /// <summary>
    /// Play item at index (or item_id) X in queue.
    /// </summary>
    public async Task PlayIndexAsync(string queueId, int index)
    {
        await SendCommandAsync<object>("player_queues/play_index", new { queue_id = queueId, index = index });
    }

    /// <summary>
    /// Delete item by index from the queue.
    /// </summary>
    public async Task DeleteQueueItemAsync(string queueId, int itemIndex)
    {
        await SendCommandAsync<object>("player_queues/delete_item", new { queue_id = queueId, item_id_or_index = itemIndex });
    }

    /// <summary>
    /// Delete item by queue item id from the queue.
    /// </summary>
    public async Task DeleteQueueItemAsync(string queueId, string itemId)
    {
        await SendCommandAsync<object>("player_queues/delete_item", new { queue_id = queueId, item_id_or_index = itemId });
    }

    /// <summary>
    /// Move queue item x up/down the queue.
    /// </summary>
    public async Task MoveQueueItemAsync(string queueId, string queueItemId, int posShift = 0)
    {
        await SendCommandAsync<object>("player_queues/move_item", new
        {
            queue_id = queueId,
            queue_item_id = queueItemId,
            pos_shift = posShift
        });
    }

    /// <summary>
    /// Configure shuffle setting on the the queue.
    /// </summary>
    public async Task SetShuffleAsync(string queueId, bool shuffleEnabled)
    {
        await SendCommandAsync<object>("player_queues/shuffle", new { queue_id = queueId, shuffle_enabled = shuffleEnabled });
    }

    /// <summary>
    /// Configure repeat setting on the the queue.
    /// </summary>
    public async Task SetRepeatAsync(string queueId, RepeatMode repeatMode)
    {
        await SendCommandAsync<object>("player_queues/repeat", new
        {
            queue_id = queueId,
            repeat_mode = repeatMode.ToString().ToLowerInvariant()
        });
    }

    /// <summary>
    /// Configure "Don't stop the music" setting on the queue.
    /// </summary>
    public async Task SetDontStopTheMusicAsync(string queueId, bool dontStopTheMusicEnabled)
    {
        await SendCommandAsync<object>("player_queues/dont_stop_the_music", new
        {
            queue_id = queueId,
            dont_stop_the_music_enabled = dontStopTheMusicEnabled
        });
    }

    #endregion

    #region Players

    /// <summary>
    /// Return PlayerState for all registered players.
    /// </summary>
    public async Task<List<Player>> GetPlayersAsync(bool returnUnavailable = false)
    {
        var args = new { return_unavailable = returnUnavailable };
        var result = await SendCommandAsync<List<Player>>("players/all", args);
        return result ?? new List<Player>();
    }

    /// <summary>
    /// Return PlayerState by player_id.
    /// </summary>
    public async Task<Player?> GetPlayerAsync(string playerId, bool raiseUnavailable = false)
    {
        var args = new { player_id = playerId, raise_unavailable = raiseUnavailable };
        return await SendCommandAsync<Player>("players/get", args);
    }

    /// <summary>
    /// Send POWER command to given player.
    /// </summary>
    public async Task SetPlayerPowerAsync(string playerId, bool powered)
    {
        await SendCommandAsync<object>("players/cmd/power", new { player_id = playerId, powered = powered });
    }

    /// <summary>
    /// Send VOLUME_SET command to given player.
    /// </summary>
    public async Task SetPlayerVolumeAsync(string playerId, int volumeLevel)
    {
        _logger.LogInformation("Set volume on player: {PlayerId} to {Volume}", playerId, volumeLevel);
        await SendCommandAsync<object>("players/cmd/volume_set", new { player_id = playerId, volume_level = volumeLevel });
    }

    /// <summary>
    /// Send VOLUME_MUTE command to given player.
    /// </summary>
    public async Task SetPlayerMuteAsync(string playerId, bool muted)
    {
        await SendCommandAsync<object>("players/cmd/volume_mute", new { player_id = playerId, muted = muted });
    }

    /// <summary>
    /// Send VOLUME_UP command to given player.
    /// </summary>
    public async Task PlayerVolumeUpAsync(string playerId)
    {
        await SendCommandAsync<object>("players/cmd/volume_up", new { player_id = playerId });
    }

    /// <summary>
    /// Send VOLUME_DOWN command to given player.
    /// </summary>
    public async Task PlayerVolumeDownAsync(string playerId)
    {
        await SendCommandAsync<object>("players/cmd/volume_down", new { player_id = playerId });
    }

    /// <summary>
    /// Send PLAY (unpause) command to given player.
    /// </summary>
    public async Task PlayerPlayAsync(string playerId)
    {
        _logger.LogInformation("Play on player: {PlayerId}", playerId);
        await SendCommandAsync<object>("players/cmd/play", new { player_id = playerId });
    }

    /// <summary>
    /// Send PAUSE command to given player.
    /// </summary>
    public async Task PlayerPauseAsync(string playerId)
    {
        _logger.LogInformation("Pause on player: {PlayerId}", playerId);
        await SendCommandAsync<object>("players/cmd/pause", new { player_id = playerId });
    }

    /// <summary>
    /// Toggle play/pause on given player.
    /// </summary>
    public async Task PlayerPlayPauseAsync(string playerId)
    {
        _logger.LogInformation("Play/Pause on player: {PlayerId}", playerId);
        await SendCommandAsync<object>("players/cmd/play_pause", new { player_id = playerId });
    }

    /// <summary>
    /// Handle NEXT TRACK command for given player.
    /// </summary>
    public async Task PlayerNextAsync(string playerId)
    {
        _logger.LogInformation("Next on player: {PlayerId}", playerId);
        await SendCommandAsync<object>("players/cmd/next", new { player_id = playerId });
    }

    /// <summary>
    /// Handle PREVIOUS TRACK command for given player.
    /// </summary>
    public async Task PlayerPreviousAsync(string playerId)
    {
        _logger.LogInformation("Previous on player: {PlayerId}", playerId);
        await SendCommandAsync<object>("players/cmd/previous", new { player_id = playerId });
    }

    /// <summary>
    /// Handle STOP command for given player.
    /// </summary>
    public async Task PlayerStopAsync(string playerId)
    {
        _logger.LogInformation("Stop on player: {PlayerId}", playerId);
        await SendCommandAsync<object>("players/cmd/stop", new { player_id = playerId });
    }

    /// <summary>
    /// Handle SEEK command for given player.
    /// </summary>
    public async Task PlayerSeekAsync(string playerId, int position)
    {
        _logger.LogInformation("Seek on player: {PlayerId} to {Position}", playerId, position);
        await SendCommandAsync<object>("players/cmd/seek", new { player_id = playerId, position = position });
    }

    #endregion

    #region Provider Enrichment and Cache Helpers

    /// <summary>
    /// Get provider manifest with caching
    /// </summary>
    private async Task<ProviderManifest?> GetCachedProviderManifestAsync(string domain)
    {
        if (string.IsNullOrEmpty(domain))
            return null;

        // Check cache first (thread-safe read)
        if (_providerManifestCache.TryGetValue(domain, out var cached))
        {
            return cached;
        }

        // Acquire lock for write
        await _manifestCacheLock.WaitAsync();
        try
        {
            // Double-check after acquiring lock
            if (_providerManifestCache.TryGetValue(domain, out cached))
            {
                return cached;
            }

            // Fetch from API
            _logger.LogDebug("Fetching provider manifest for: {Domain}", domain);
            var manifest = await GetProviderManifestAsync(domain);

            if (manifest != null)
            {
                _providerManifestCache[domain] = manifest;
                _logger.LogDebug("Cached provider manifest: {Domain} - {Name}", domain, manifest.Name);
            }

            return manifest;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to fetch provider manifest for: {Domain}", domain);
            return null;
        }
        finally
        {
            _manifestCacheLock.Release();
        }
    }

    /// <summary>
    /// Enrich media items with provider manifest for their primary provider
    /// </summary>
    public async Task EnrichWithProviderInfoAsync<T>(IEnumerable<T> items) where T : MediaItem
    {
        if (items == null)
            return;

        var materialized = items as List<T> ?? items.ToList();
        if (materialized.Count == 0)
            return;

        // Sammle alle einzigartigen primären Provider (aus ProviderMappings)
        var uniqueProviders = materialized
            .Select(item => item.ProviderMappings.FirstOrDefault()?.ProviderDomain)
            .Where(p => !string.IsNullOrEmpty(p))
            .Select(p => p!)
            .Distinct()
            .ToList();

        _logger.LogDebug("Fetching {Count} unique provider manifests for {ItemCount} items",
            uniqueProviders.Count, materialized.Count);

        // Lade alle Manifeste parallel (nur einmal pro Provider dank Cache)
        var manifestResults = await Task.WhenAll(
            uniqueProviders.Select(async p => new
            {
                Domain = p,
                Manifest = await GetCachedProviderManifestAsync(p)
            }));

        var manifestByDomain = manifestResults
            .Where(r => r.Manifest != null)
            .ToDictionary(r => r.Domain, r => r.Manifest!);

        // Setze das Manifest für jedes Item
        foreach (var item in materialized)
        {
            ResolveMediaItemImages(item);

            var providerDomain = item.ProviderMappings.FirstOrDefault()?.ProviderDomain;
            if (!string.IsNullOrEmpty(providerDomain) &&
                manifestByDomain.TryGetValue(providerDomain, out var manifest))
            {
                item.ProviderManifest = manifest;
            }
        }
    }

    /// <summary>
    /// Return all Provider manifests.
    /// </summary>
    public async Task<List<ProviderManifest>> GetProviderManifestsAsync()
    {
        _logger.LogInformation("Fetching provider manifests...");

        var result = await SendCommandAsync<List<ProviderManifest>>("providers/manifests");
        return result ?? new List<ProviderManifest>();
    }

    /// <summary>
    /// Return Provider manifest of single provider (domain).
    /// </summary>
    /// <param name="domain">The provider domain (e.g., "spotify", "qobuz", "ytmusic")</param>
    public async Task<ProviderManifest?> GetProviderManifestAsync(string domain)
    {
        _logger.LogInformation("Fetching provider manifest for: {Domain}", domain);

        var args = new { instance_id_or_domain = domain };
        return await SendCommandAsync<ProviderManifest>("providers/manifests/get", args);
    }

    #endregion

    #region Image Helpers

    private static string RestoreImagePath(string? path, string baseUrl)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return string.Empty;
        }

        if (!Uri.TryCreate(path, UriKind.Absolute, out var uri))
        {
            return path;
        }

        var proxyPath = $"{baseUrl.TrimEnd('/')}/imageproxy";
        if (!path.StartsWith(proxyPath, StringComparison.OrdinalIgnoreCase))
        {
            return path;
        }

        var query = uri.Query.TrimStart('?');
        foreach (var part in query.Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var kvp = part.Split('=', 2);
            if (kvp.Length == 2 && kvp[0] == "path")
            {
                return WebUtility.UrlDecode(kvp[1]);
            }
        }

        return path;
    }

    private string ResolveImagePath(string? path, string? proxyId = null, string? checksum = null)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return string.Empty;
        }

        if (!string.IsNullOrWhiteSpace(proxyId))
        {
            var baseUrl = _settings.MusicAssistantUrl.TrimEnd('/');
            if (!string.IsNullOrWhiteSpace(checksum))
            {
                return string.Concat(baseUrl, "/imageproxy/", proxyId, "?checksum=", Uri.EscapeDataString(checksum));
            }

            return string.Concat(baseUrl, "/imageproxy/", proxyId);
        }

        return path;
    }

    private void ResolveMediaItemImages(MediaItem item)
    {
        ResolveMetadataImages(item.Metadata);

        if (item is Track track)
        {
            if (track.Album != null)
            {
                ResolveMediaItemImages(track.Album);
            }
        }
    }

    private void ResolveMediaItemImages(IEnumerable<MediaItem> items)
    {
        foreach (var item in items)
        {
            if (item == null)
            {
                continue;
            }

            ResolveMediaItemImages(item);
        }
    }

    private void ResolveMetadataImages(MediaItemMetadata? metadata)
    {
        if (metadata?.Images == null || metadata.Images.Count == 0)
        {
            return;
        }

        foreach (var image in metadata.Images)
        {
            if (image == null)
            {
                continue;
            }

            image.Path = ResolveImagePath(image.Path, image.ProxyId, metadata.CacheChecksum);
        }
    }

    private Task<T?> DeserializeMediaItemAsync<T>(JsonElement element) where T : MediaItem
    {
        var item = JsonSerializer.Deserialize<T>(element.GetRawText(), JsonOptions);
        if (item != null)
        {
            _ = EnrichWithProviderInfoAsync(new List<T> { item });
        }

        return Task.FromResult(item);
    }

    #endregion

}