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
    private readonly ILogger<MusicAssistantService> _logger;
    private readonly SettingsService _settings;
    private readonly HttpClient _httpClient;

    private string? _authToken;
    private bool _isAuthenticated;

    // Provider Manifest Cache
    private readonly Dictionary<string, ProviderManifest> _providerManifestCache = new();
    private readonly SemaphoreSlim _manifestCacheLock = new(1, 1);

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        Converters =
        {
            new JsonStringEnumConverter(),
            new FlexibleIntConverter()
        }
    };

    public bool IsAuthenticated => _isAuthenticated;

    /// <summary>
    /// Event triggered when login is required (no valid token available).
    /// Subscribe to this event to show a login UI.
    /// </summary>
    public event EventHandler? LoginRequired;

    public MusicAssistantService(
        ILogger<MusicAssistantService> logger,
        SettingsService settings)
    {
        _logger = logger;
        _settings = settings;
        _httpClient = new HttpClient
        {
            Timeout = TimeSpan.FromSeconds(60),
            BaseAddress = new Uri(settings.MusicAssistantUrl)
        };
    }

    private async Task<T?> SendCommandAsync<T>(string command, object? args = null)
    {
        if (!_isAuthenticated && command != "auth/login")
        {
            var autoLoginSuccess = await TryAutoLoginAsync();
            if (!autoLoginSuccess)
            {
                throw new UnauthorizedAccessException("Not authenticated. Please login first.");
            }
        }

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
            var response = await _httpClient.PostAsync("/api", content);

            if (response.StatusCode == System.Net.HttpStatusCode.Unauthorized)
            {
                _logger.LogWarning("Unauthorized - token may have expired");
                Logout();
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
                _authToken = response.Token;
                _isAuthenticated = true;

                _httpClient.DefaultRequestHeaders.Authorization =
                    new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", _authToken);

                _logger.LogInformation("Successfully authenticated as: {Username}", response.User?.Username ?? username);

                _settings.AuthToken = _authToken;
                _settings.Username = username;
                _settings.Save();

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
            if (_isAuthenticated)
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
    /// Set an existing Auth-Token from settings (internal use)
    /// </summary>
    public void SetAuthToken(string token)
    {
        _authToken = token;
        _isAuthenticated = !string.IsNullOrEmpty(token);

        if (_isAuthenticated)
        {
            _httpClient.DefaultRequestHeaders.Authorization =
                new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token);

            _logger.LogInformation("Auth token set from saved credentials");
        }
    }

    /// <summary>
    /// Try Auto-Login with saved Token in settings (internal use)
    /// </summary>
    public async Task<bool> TryAutoLoginAsync()
    {
        if (!string.IsNullOrEmpty(_settings.AuthToken))
        {
            _logger.LogInformation("Attempting auto-login with saved token...");
            SetAuthToken(_settings.AuthToken);

            try
            {
                await GetServerInfoAsync();
                _logger.LogInformation("Auto-login successful");
                return true;
            }
            catch
            {
                _logger.LogWarning("Saved token invalid, clearing...");
                Logout();
            }
        }

        // No valid token available - notify that login is required
        _logger.LogInformation("No valid auth token available, login required");
        LoginRequired?.Invoke(this, EventArgs.Empty);
        return false;
    }

    /// <summary>
    /// Logout (internal use)
    /// </summary>
    public void Logout()
    {
        _authToken = null;
        _isAuthenticated = false;
        _httpClient.DefaultRequestHeaders.Authorization = null;

        _settings.AuthToken = null;
        _settings.Save();

        _logger.LogInformation("Logged out");
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

        return await SendCommandAsync<T>("music/get_library_item", args);
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
        return result ?? new List<Artist>();
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
            await EnrichWithProviderInfoAsync(new List<Artist> { artist });
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

        await EnrichWithProviderInfoAsync(albums);
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
        await EnrichWithProviderInfoAsync(tracks);
        return result ?? new List<Track>();
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
        return result ?? new List<Album>();
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
            await EnrichWithProviderInfoAsync(new List<Album> { album });
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

        await EnrichWithProviderInfoAsync(tracks);
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

        // Enrich with provider information
        await EnrichWithProviderInfoAsync(tracks);

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

        return await SendCommandAsync<Track>("music/tracks/get", args);
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
        return result ?? new List<Track>();
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
        return result ?? new List<Album>();
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

        // Enrich with provider information
        await EnrichWithProviderInfoAsync(tracks);

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
        bool libraryItemsOnly = true)
    {
        _logger.LogInformation("Fetching library playlists...");

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
        return result ?? new List<Playlist>();
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

        return await SendCommandAsync<Playlist>("music/playlists/get", args);
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

        // Enrich with provider information
        await EnrichWithProviderInfoAsync(tracks);
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
            var enrichTasks = new List<Task>
            {
                EnrichWithProviderInfoAsync(results.Tracks ?? new List<Track>()),
                EnrichWithProviderInfoAsync(results.Albums ?? new List<Album>()),
                EnrichWithProviderInfoAsync(results.Artists ?? new List<Artist>()),
                EnrichWithProviderInfoAsync(results.Playlists ?? new List<Playlist>())
            };

            await Task.WhenAll(enrichTasks);
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
    /// Return a list of the last added tracks.
    /// </summary>
    public async Task<List<Track>> GetRecentlyAddedTracksAsync(int limit = 50)
    {
        var args = new { limit = limit };
        var result = await SendCommandAsync<List<Track>>("music/recently_added_tracks", args);
        return result ?? new List<Track>();
    }

    /// <summary>
    /// Return a list of the last played items. Returns various media types
    /// </summary>
    public async Task<List<MediaItem>> GetRecentlyPlayedItemsAsync(int limit = 50, MediaType[]? mediaTypes = null)
    {
        var args = new Dictionary<string, object>
        {
            ["limit"] = limit
        };

        if (mediaTypes != null && mediaTypes.Length > 0)
        {
            args["media_types"] = mediaTypes.Select(mt => mt.ToString().ToLowerInvariant()).ToList();
        }

        var result = await SendCommandAsync<List<MediaItem>>("music/recently_played_items", args);
        return result ?? new List<MediaItem>();
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
        _logger.LogInformation("Fetching active queue for player: {PlayerId}", playerId);

        var args = new { player_id = playerId };
        return await SendCommandAsync<PlayerQueue>("player_queues/get_active_queue", args);
    }

    /// <summary>
    /// Return all QueueItems for given PlayerQueue.
    /// </summary>
    public async Task<List<QueueItem>> GetQueueItemsAsync(string queueId, int? limit = null, int? offset = null)
    {
        _logger.LogInformation("Fetching queue items for: {QueueId}", queueId);

        var args = new Dictionary<string, object>
        {
            ["queue_id"] = queueId
        };

        if (limit.HasValue) args["limit"] = limit.Value;
        if (offset.HasValue) args["offset"] = offset.Value;

        var result = await SendCommandAsync<List<QueueItem>>("player_queues/items", args);
        return result ?? new List<QueueItem>();
    }

    /// <summary>
    /// Play media items on the given queue.
    /// </summary>
    public async Task PlayMediaAsync(
     string queueId,
     List<MediaItem> mediaItems,
     QueueOption option = QueueOption.Replace,
     bool radioMode = false)
    {
        if (mediaItems == null || mediaItems.Count == 0)
        {
            throw new ArgumentException("Media items list cannot be null or empty", nameof(mediaItems));
        }

        var uris = mediaItems
            .Where(item => !string.IsNullOrEmpty(item.Uri))
            .Select(item => item.Uri!)
            .ToArray();

        if (uris.Length == 0)
        {
            throw new ArgumentException("No valid URIs found in media items", nameof(mediaItems));
        }

        _logger.LogInformation("Playing {Count} media item(s) on queue: {QueueId}", uris.Length, queueId);

        var args = new Dictionary<string, object>
        {
            ["queue_id"] = queueId,
            ["media"] = uris,
            ["option"] = option.ToString().ToLowerInvariant(),
            ["radio_mode"] = radioMode
        };

        await SendCommandAsync<object>("player_queues/play_media", args);
    }

    /// <summary>
    /// Play a single media item on the given queue.
    /// </summary>
    public async Task PlayMediaAsync(
        string queueId,
        MediaItem mediaItem,
        QueueOption option = QueueOption.Play,
        bool radioMode = false)
    {
        await PlayMediaAsync(queueId, new List<MediaItem> { mediaItem }, option, radioMode);
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
    /// Delete item (by id or index) from the queue.
    /// </summary>
    public async Task DeleteQueueItemAsync(string queueId, string itemIdOrIndex)
    {
        await SendCommandAsync<object>("player_queues/delete_item", new { queue_id = queueId, item_id_or_index = itemIdOrIndex });
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

    #region Provider Manifest Caching

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
    private async Task EnrichWithProviderInfoAsync<T>(List<T> items) where T : MediaItem
    {
        if (items == null || items.Count == 0)
            return;

        // Sammle alle einzigartigen primären Provider (aus ProviderMappings)
        var uniqueProviders = items
            .Select(item => item.ProviderMappings.FirstOrDefault()?.ProviderDomain)
            .Where(p => !string.IsNullOrEmpty(p))
            .Select(p => p!)
            .Distinct()
            .ToList();

        _logger.LogDebug("Fetching {Count} unique provider manifests for {ItemCount} items",
            uniqueProviders.Count, items.Count);

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
        foreach (var item in items)
        {
            var providerDomain = item.ProviderMappings.FirstOrDefault()?.ProviderDomain;
            if (!string.IsNullOrEmpty(providerDomain) &&
                manifestByDomain.TryGetValue(providerDomain, out var manifest))
            {
                item.ProviderManifest = manifest;
            }
        }
    }

    #endregion

    #region Providers

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

        var args = new { domain = domain };
        return await SendCommandAsync<ProviderManifest>("providers/manifests/get", args);
    }

    

    #endregion

}