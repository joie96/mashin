using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.Extensions.Logging;
using mashin.Models;

namespace mashin.Services;

/// <summary>
/// Client for the ListenBrainz REST API (https://api.listenbrainz.org).
/// Implements core, stats, and popularity endpoints.
/// </summary>
public class ListenBrainzService
{
    private const string BaseUrl = "https://api.listenbrainz.org";
    private const string UserAgent = "mashin/1.0 (https://github.com/ericb)";

    private readonly ILogger<ListenBrainzService> _logger;
    private readonly SettingsService _settings;
    private readonly HttpClient _httpClient;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        Converters = { new JsonStringEnumConverter() }
    };

    #region Static helpers

    /// <summary>
    /// Returns a Cover Art Archive front-cover URL for the given release MBID.
    /// Size: 250, 500 (default) or 1200.
    /// </summary>
    public static string CoverArtUrl(string releaseMbid, int size = 500) =>
        $"https://coverartarchive.org/release/{releaseMbid}/front-{size}";

    /// <summary>
    /// Returns the range string used by the ListenBrainz API for a given <see cref="LbStatRange"/>.
    /// </summary>
    public static string ToApiRange(LbStatRange range) => range switch
    {
        LbStatRange.ThisWeek   => "this_week",
        LbStatRange.ThisMonth  => "this_month",
        LbStatRange.ThisYear   => "this_year",
        LbStatRange.Week       => "week",
        LbStatRange.Month      => "month",
        LbStatRange.Quarter    => "quarter",
        LbStatRange.HalfYearly => "half_yearly",
        LbStatRange.Year       => "year",
        LbStatRange.AllTime    => "all_time",
        _                      => "all_time"
    };

    #endregion
    #region Construction

    public ListenBrainzService(
        ILogger<ListenBrainzService> logger,
        SettingsService settings)
    {
        _logger = logger;
        _settings = settings;

        _httpClient = new HttpClient
        {
            BaseAddress = new Uri(BaseUrl),
            Timeout = TimeSpan.FromSeconds(30)
        };
        _httpClient.DefaultRequestHeaders.UserAgent.ParseAdd(UserAgent);
    }

    #endregion
    #region Auth helpers

    private void ApplyAuthHeader(HttpRequestMessage request, string? token)
    {
        if (!string.IsNullOrWhiteSpace(token))
            request.Headers.Authorization = new AuthenticationHeaderValue("Token", token);
    }

    #endregion
    #region Low-level GET / POST

    private async Task<T?> GetAsync<T>(string path, string? token = null,
        CancellationToken ct = default)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, path);
        ApplyAuthHeader(request, token);

        try
        {
            var response = await _httpClient.SendAsync(request, ct);

            if (response.StatusCode == HttpStatusCode.NoContent)
                return default;

            response.EnsureSuccessStatusCode();

            var json = await response.Content.ReadAsStringAsync(ct);
            _logger.LogTrace("GET {Path} → {Json}", path, json);
            return JsonSerializer.Deserialize<T>(json, JsonOptions);
        }
        catch (HttpRequestException ex)
        {
            _logger.LogError(ex, "HTTP error on GET {Path}", path);
            throw;
        }
    }

    private async Task<T?> PostAsync<T>(string path, object body, string? token = null,
        CancellationToken ct = default)
    {
        var json = JsonSerializer.Serialize(body, JsonOptions);
        using var request = new HttpRequestMessage(HttpMethod.Post, path)
        {
            Content = new StringContent(json, Encoding.UTF8, "application/json")
        };
        ApplyAuthHeader(request, token);

        try
        {
            var response = await _httpClient.SendAsync(request, ct);
            response.EnsureSuccessStatusCode();

            var responseJson = await response.Content.ReadAsStringAsync(ct);
            _logger.LogTrace("POST {Path} → {Json}", path, responseJson);
            return JsonSerializer.Deserialize<T>(responseJson, JsonOptions);
        }
        catch (HttpRequestException ex)
        {
            _logger.LogError(ex, "HTTP error on POST {Path}", path);
            throw;
        }
    }

    #endregion
    #region Token Validation

    /// <summary>
    /// Validates a ListenBrainz user token.
    /// Returns <c>null</c> on network / server error.
    /// </summary>
    public async Task<LbValidateTokenResponse?> ValidateTokenAsync(
        string token, CancellationToken ct = default)
    {
        return await GetAsync<LbValidateTokenResponse>(
            "/1/validate-token", token, ct);
    }

    #endregion
    #region Core: Listens

    /// <summary>
    /// Gets the listen history for <paramref name="userName"/>.
    /// </summary>
    /// <param name="userName">ListenBrainz username.</param>
    /// <param name="count">Number of listens to return (max 100, default 25).</param>
    /// <param name="maxTs">Return listens older than this Unix timestamp.</param>
    /// <param name="minTs">Return listens newer than this Unix timestamp.</param>
    public async Task<LbListensResponse?> GetListensAsync(
        string userName,
        int count = 25,
        long? maxTs = null,
        long? minTs = null,
        CancellationToken ct = default)
    {
        var query = new StringBuilder($"/1/user/{Uri.EscapeDataString(userName)}/listens?count={count}");
        if (maxTs.HasValue) query.Append($"&max_ts={maxTs}");
        if (minTs.HasValue) query.Append($"&min_ts={minTs}");

        return await GetAsync<LbListensResponse>(query.ToString(), ct: ct);
    }

    /// <summary>
    /// Gets the total number of listens for <paramref name="userName"/>.
    /// </summary>
    public async Task<long?> GetListenCountAsync(
        string userName, CancellationToken ct = default)
    {
        var result = await GetAsync<LbListenCountResponse>(
            $"/1/user/{Uri.EscapeDataString(userName)}/listen-count", ct: ct);
        return result?.Payload.Count;
    }

    /// <summary>
    /// Gets the track that <paramref name="userName"/> is currently playing.
    /// Returns <c>null</c> if nothing is playing or the user has no "playing now" data.
    /// </summary>
    public async Task<LbPlayingNowItem?> GetPlayingNowAsync(
        string userName, CancellationToken ct = default)
    {
        var result = await GetAsync<LbPlayingNowResponse>(
            $"/1/user/{Uri.EscapeDataString(userName)}/playing-now", ct: ct);
        return result?.NowPlaying;
    }

    #endregion
    #region Core: Submit Listens

    /// <summary>
    /// Submits a single "playing now" notification to ListenBrainz.
    /// Requires a valid <paramref name="token"/>.
    /// </summary>
    public Task SubmitPlayingNowAsync(
        string token,
        LbTrackMetadata trackMetadata,
        CancellationToken ct = default)
    {
        var payload = new LbSubmitListenRequest(
            "playing_now",
            [new LbSubmitListenPayloadItem(null, trackMetadata)]
        );
        return PostAsync<object>("/1/submit-listens", payload, token, ct);
    }

    /// <summary>
    /// Submits a single completed listen to ListenBrainz.
    /// Requires a valid <paramref name="token"/>.
    /// </summary>
    public Task SubmitListenAsync(
        string token,
        LbTrackMetadata trackMetadata,
        DateTimeOffset listenedAt,
        CancellationToken ct = default)
    {
        var payload = new LbSubmitListenRequest(
            "single",
            [new LbSubmitListenPayloadItem(listenedAt.ToUnixTimeSeconds(), trackMetadata)]
        );
        return PostAsync<object>("/1/submit-listens", payload, token, ct);
    }

    /// <summary>
    /// Bulk-imports a list of listens (up to 1000 per request).
    /// Requires a valid <paramref name="token"/>.
    /// </summary>
    public Task ImportListensAsync(
        string token,
        IReadOnlyList<LbSubmitListenPayloadItem> listens,
        CancellationToken ct = default)
    {
        var payload = new LbSubmitListenRequest("import", listens);
        return PostAsync<object>("/1/submit-listens", payload, token, ct);
    }

    #endregion
    #region Stats: Top Artists

    /// <summary>
    /// Returns the top artists for <paramref name="userName"/> in the given time range.
    /// Returns <c>null</c> if statistics have not yet been computed (HTTP 204).
    /// </summary>
    public async Task<LbTopArtistsPayload?> GetTopArtistsAsync(
        string userName,
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/artists" +
                   $"?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopArtistsResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Top Releases

    /// <summary>
    /// Returns the top releases for <paramref name="userName"/> in the given time range.
    /// </summary>
    public async Task<LbTopReleasesPayload?> GetTopReleasesAsync(
        string userName,
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/releases" +
                   $"?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopReleasesResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Top Release Groups

    /// <summary>
    /// Returns the top release groups (albums) for <paramref name="userName"/>.
    /// Release groups include a <see cref="LbReleaseGroupStat.CoverArtUrl"/> when
    /// a Cover Art Archive entry exists.
    /// </summary>
    public async Task<LbTopReleaseGroupsPayload?> GetTopReleaseGroupsAsync(
        string userName,
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/release-groups" +
                   $"?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopReleaseGroupsResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Top Recordings

    /// <summary>
    /// Returns the top recordings (tracks) for <paramref name="userName"/>.
    /// </summary>
    public async Task<LbTopRecordingsPayload?> GetTopRecordingsAsync(
        string userName,
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/recordings" +
                   $"?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopRecordingsResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Listening Activity

    /// <summary>
    /// Returns the listening activity (listen count per time bucket) for <paramref name="userName"/>.
    /// </summary>
    public async Task<LbListeningActivityPayload?> GetListeningActivityAsync(
        string userName,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/listening-activity" +
                   $"?range={ToApiRange(range)}";
        var result = await GetAsync<LbListeningActivityResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Daily Activity

    /// <summary>
    /// Returns the daily activity (hour-based listen distribution per weekday) for <paramref name="userName"/>.
    /// </summary>
    public async Task<LbDailyActivityPayload?> GetDailyActivityAsync(
        string userName,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/daily-activity" +
                   $"?range={ToApiRange(range)}";
        var result = await GetAsync<LbDailyActivityResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Artist Map

    /// <summary>
    /// Returns the artist map for <paramref name="userName"/>.
    /// </summary>
    public async Task<LbArtistMapPayload?> GetArtistMapAsync(
        string userName,
        LbStatRange range = LbStatRange.AllTime,
        bool forceRecalculate = false,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/user/{Uri.EscapeDataString(userName)}/artist-map" +
                   $"?range={ToApiRange(range)}&force_recalculate={forceRecalculate.ToString().ToLowerInvariant()}";
        var result = await GetAsync<LbArtistMapResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Top Listeners

    /// <summary>
    /// Returns top listeners for an artist MBID.
    /// </summary>
    public async Task<LbArtistListenersPayload?> GetTopListenersForArtistAsync(
        string artistMbid,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/artist/{Uri.EscapeDataString(artistMbid)}/listeners" +
                   $"?range={ToApiRange(range)}";
        var result = await GetAsync<LbArtistListenersResponse>(path, ct: ct);
        return result?.Payload;
    }

    /// <summary>
    /// Returns top listeners for a release-group MBID.
    /// </summary>
    public async Task<LbReleaseGroupListenersPayload?> GetTopListenersForReleaseGroupAsync(
        string releaseGroupMbid,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/release-group/{Uri.EscapeDataString(releaseGroupMbid)}/listeners" +
                   $"?range={ToApiRange(range)}";
        var result = await GetAsync<LbReleaseGroupListenersResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Stats: Sitewide

    /// <summary>
    /// Returns sitewide top artists (global chart).
    /// </summary>
    public async Task<LbTopArtistsPayload?> GetSitewideTopArtistsAsync(
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/sitewide/artists?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopArtistsResponse>(path, ct: ct);
        return result?.Payload;
    }

    /// <summary>
    /// Returns sitewide top releases.
    /// </summary>
    public async Task<LbTopReleasesPayload?> GetSitewideTopReleasesAsync(
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/sitewide/releases?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopReleasesResponse>(path, ct: ct);
        return result?.Payload;
    }

    /// <summary>
    /// Returns sitewide top release groups.
    /// </summary>
    public async Task<LbTopReleaseGroupsPayload?> GetSitewideTopReleaseGroupsAsync(
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/sitewide/release-groups?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopReleaseGroupsResponse>(path, ct: ct);
        return result?.Payload;
    }

    /// <summary>
    /// Returns sitewide top recordings.
    /// </summary>
    public async Task<LbTopRecordingsPayload?> GetSitewideTopRecordingsAsync(
        int count = 25,
        int offset = 0,
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/sitewide/recordings?count={count}&offset={offset}&range={ToApiRange(range)}";
        var result = await GetAsync<LbTopRecordingsResponse>(path, ct: ct);
        return result?.Payload;
    }

    /// <summary>
    /// Returns sitewide listening activity.
    /// </summary>
    public async Task<LbListeningActivityPayload?> GetSitewideListeningActivityAsync(
        LbStatRange range = LbStatRange.AllTime,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/sitewide/listening-activity?range={ToApiRange(range)}";
        var result = await GetAsync<LbListeningActivityResponse>(path, ct: ct);
        return result?.Payload;
    }

    /// <summary>
    /// Returns sitewide artist map.
    /// </summary>
    public async Task<LbArtistMapPayload?> GetSitewideArtistMapAsync(
        LbStatRange range = LbStatRange.AllTime,
        bool forceRecalculate = false,
        CancellationToken ct = default)
    {
        var path = $"/1/stats/sitewide/artist-map?range={ToApiRange(range)}" +
                   $"&force_recalculate={forceRecalculate.ToString().ToLowerInvariant()}";
        var result = await GetAsync<LbArtistMapResponse>(path, ct: ct);
        return result?.Payload;
    }

    #endregion
    #region Popularity: Top Recordings for Artist

    /// <summary>
    /// Returns the most popular recordings for an artist, identified by their MusicBrainz ID.
    /// Each result includes a <see cref="LbTopRecordingForArtist.CoverArtUrl"/> when available.
    /// </summary>
    public async Task<IReadOnlyList<LbTopRecordingForArtist>> GetTopRecordingsForArtistAsync(
        string artistMbid, CancellationToken ct = default)
    {
        var result = await GetAsync<IReadOnlyList<LbTopRecordingForArtist>>(
            $"/1/popularity/top-recordings-for-artist/{Uri.EscapeDataString(artistMbid)}", ct: ct);
        return result ?? [];
    }

    #endregion
    #region Popularity: Top Release Groups for Artist

    /// <summary>
    /// Returns the most popular release groups (albums) for an artist identified by their MBID.
    /// </summary>
    public async Task<IReadOnlyList<LbTopReleaseGroupForArtist>> GetTopReleaseGroupsForArtistAsync(
        string artistMbid, CancellationToken ct = default)
    {
        var result = await GetAsync<IReadOnlyList<LbTopReleaseGroupForArtist>>(
            $"/1/popularity/top-release-groups-for-artist/{Uri.EscapeDataString(artistMbid)}", ct: ct);
        return result ?? [];
    }

    #endregion
    #region Social: Similar Users

    /// <summary>
    /// Returns users with a similar listening taste to <paramref name="userName"/>.
    /// </summary>
    public async Task<IReadOnlyList<LbSimilarUser>> GetSimilarUsersAsync(
        string userName, CancellationToken ct = default)
    {
        var result = await GetAsync<LbSimilarUsersResponse>(
            $"/1/user/{Uri.EscapeDataString(userName)}/similar-users", ct: ct);
        return result?.Payload ?? [];
    }

    #endregion
}
