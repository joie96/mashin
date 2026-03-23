using System.Text.Json.Serialization;

namespace mashin.Models;

#region Enums

public enum LbStatRange
{
    [JsonPropertyName("this_week")]   ThisWeek,
    [JsonPropertyName("this_month")]  ThisMonth,
    [JsonPropertyName("this_year")]   ThisYear,
    [JsonPropertyName("week")]        Week,
    [JsonPropertyName("month")]       Month,
    [JsonPropertyName("quarter")]     Quarter,
    [JsonPropertyName("half_yearly")] HalfYearly,
    [JsonPropertyName("year")]        Year,
    [JsonPropertyName("all_time")]    AllTime
}

#endregion

#region Shared

public record LbArtistCredit(
    [property: JsonPropertyName("artist_credit_name")] string? ArtistCreditName,
    [property: JsonPropertyName("join_phrase")]        string? JoinPhrase,
    [property: JsonPropertyName("artist_mbid")]        string? ArtistMbid,
    [property: JsonPropertyName("artist_name")]        string? ArtistName,
    [property: JsonPropertyName("listen_count")]       int?    ListenCount
);

#endregion

#region Token Validation

public record LbValidateTokenResponse(
    [property: JsonPropertyName("code")]     int    Code,
    [property: JsonPropertyName("message")] string? Message,
    [property: JsonPropertyName("valid")]   bool    Valid,
    [property: JsonPropertyName("user_name")] string? UserName
);

#endregion

#region Submit Listens

public record LbSubmitListenRequest(
    [property: JsonPropertyName("listen_type")] string ListenType,   // "single", "playing_now", "import"
    [property: JsonPropertyName("payload")]     IReadOnlyList<LbSubmitListenPayloadItem> Payload
);

public record LbSubmitListenPayloadItem(
    [property: JsonPropertyName("listened_at")]    long?             ListenedAt,   // Unix timestamp; null for playing_now
    [property: JsonPropertyName("track_metadata")] LbTrackMetadata   TrackMetadata
);

#endregion

#region Track Metadata

public record LbTrackMetadata(
    [property: JsonPropertyName("artist_name")]      string  ArtistName,
    [property: JsonPropertyName("track_name")]       string  TrackName,
    [property: JsonPropertyName("release_name")]     string? ReleaseName,
    [property: JsonPropertyName("additional_info")]  LbAdditionalInfo? AdditionalInfo,
    [property: JsonPropertyName("mbid_mapping")]     LbMbidMapping?    MbidMapping
);

public record LbAdditionalInfo(
    [property: JsonPropertyName("recording_mbid")]    string?                  RecordingMbid,
    [property: JsonPropertyName("release_mbid")]      string?                  ReleaseMbid,
    [property: JsonPropertyName("artist_mbids")]      IReadOnlyList<string>?   ArtistMbids,
    [property: JsonPropertyName("isrc")]              string?                  Isrc,
    [property: JsonPropertyName("tracknumber")]       int?                     TrackNumber,
    [property: JsonPropertyName("discnumber")]        int?                     DiscNumber,
    [property: JsonPropertyName("duration_ms")]       int?                     DurationMs,
    [property: JsonPropertyName("media_player")]      string?                  MediaPlayer,
    [property: JsonPropertyName("submission_client")] string?                  SubmissionClient,
    [property: JsonPropertyName("music_service")]     string?                  MusicService,
    [property: JsonPropertyName("origin_url")]        string?                  OriginUrl
);

public record LbMbidMapping(
    [property: JsonPropertyName("recording_mbid")]      string?                  RecordingMbid,
    [property: JsonPropertyName("release_mbid")]        string?                  ReleaseMbid,
    [property: JsonPropertyName("release_group_mbid")]  string?                  ReleaseGroupMbid,
    [property: JsonPropertyName("artist_mbids")]        IReadOnlyList<string>?   ArtistMbids,
    [property: JsonPropertyName("caa_id")]              long?                    CaaId,
    [property: JsonPropertyName("caa_release_mbid")]    string?                  CaaReleaseMbid,
    [property: JsonPropertyName("artists")]             IReadOnlyList<LbArtistCredit>? Artists
);

#endregion

#region Listens

public record LbListen(
    [property: JsonPropertyName("listened_at")]    long            ListenedAt,
    [property: JsonPropertyName("user_name")]      string?         UserName,
    [property: JsonPropertyName("recording_msid")] string?         RecordingMsid,
    [property: JsonPropertyName("track_metadata")] LbTrackMetadata TrackMetadata
)
{
    public DateTimeOffset ListenedAtUtc => DateTimeOffset.FromUnixTimeSeconds(ListenedAt);
}

public record LbListensPayload(
    [property: JsonPropertyName("listens")]      IReadOnlyList<LbListen> Listens,
    [property: JsonPropertyName("count")]        int                     Count,
    [property: JsonPropertyName("oldest_listen_ts")] long?               OldestListenTs,
    [property: JsonPropertyName("latest_listen_ts")] long?               LatestListenTs,
    [property: JsonPropertyName("user_id")]      string?                 UserId
);

public record LbListensResponse(
    [property: JsonPropertyName("payload")] LbListensPayload Payload
);

#endregion

#region Playing Now

public record LbPlayingNowItem(
    [property: JsonPropertyName("playing_now")]    bool            PlayingNow,
    [property: JsonPropertyName("track_metadata")] LbTrackMetadata TrackMetadata
);

public record LbPlayingNowPayload(
    [property: JsonPropertyName("listens")] IReadOnlyList<LbPlayingNowItem> Listens,
    [property: JsonPropertyName("count")]   int                             Count,
    [property: JsonPropertyName("user_id")] string?                         UserId
);

public record LbPlayingNowResponse(
    [property: JsonPropertyName("payload")] LbPlayingNowPayload Payload
)
{
    public LbPlayingNowItem? NowPlaying => Payload.Listens.Count > 0 ? Payload.Listens[0] : null;
}

#endregion

#region Listen Count

public record LbListenCountPayload(
    [property: JsonPropertyName("count")] long Count
);

public record LbListenCountResponse(
    [property: JsonPropertyName("payload")] LbListenCountPayload Payload
);

#endregion

#region Top Artists

public record LbArtistStat(
    [property: JsonPropertyName("artist_name")]  string  ArtistName,
    [property: JsonPropertyName("artist_mbid")]  string? ArtistMbid,
    [property: JsonPropertyName("listen_count")] int     ListenCount
);

public record LbTopArtistsPayload(
    [property: JsonPropertyName("artists")]            IReadOnlyList<LbArtistStat> Artists,
    [property: JsonPropertyName("count")]              int     Count,
    [property: JsonPropertyName("total_artist_count")] int     TotalArtistCount,
    [property: JsonPropertyName("offset")]             int     Offset,
    [property: JsonPropertyName("range")]              string? Range,
    [property: JsonPropertyName("from_ts")]            long?   FromTs,
    [property: JsonPropertyName("to_ts")]              long?   ToTs,
    [property: JsonPropertyName("last_updated")]       long?   LastUpdated,
    [property: JsonPropertyName("user_id")]            string? UserId
);

public record LbTopArtistsResponse(
    [property: JsonPropertyName("payload")] LbTopArtistsPayload Payload
);

#endregion

#region Top Releases

public record LbReleaseStat(
    [property: JsonPropertyName("release_name")]     string  ReleaseName,
    [property: JsonPropertyName("release_mbid")]     string? ReleaseMbid,
    [property: JsonPropertyName("artist_name")]      string? ArtistName,
    [property: JsonPropertyName("artist_mbids")]     IReadOnlyList<string>?        ArtistMbids,
    [property: JsonPropertyName("artists")]          IReadOnlyList<LbArtistCredit>? Artists,
    [property: JsonPropertyName("listen_count")]     int     ListenCount,
    [property: JsonPropertyName("caa_id")]           long?   CaaId,
    [property: JsonPropertyName("caa_release_mbid")] string? CaaReleaseMbid
)
{
    /// <summary>Cover Art Archive URL (500px), or null if no cover art available.</summary>
    public string? CoverArtUrl => CaaReleaseMbid is not null
        ? $"https://coverartarchive.org/release/{CaaReleaseMbid}/front-500"
        : null;
}

public record LbTopReleasesPayload(
    [property: JsonPropertyName("releases")]            IReadOnlyList<LbReleaseStat> Releases,
    [property: JsonPropertyName("count")]               int     Count,
    [property: JsonPropertyName("total_release_count")] int     TotalReleaseCount,
    [property: JsonPropertyName("offset")]              int     Offset,
    [property: JsonPropertyName("range")]               string? Range,
    [property: JsonPropertyName("from_ts")]             long?   FromTs,
    [property: JsonPropertyName("to_ts")]               long?   ToTs,
    [property: JsonPropertyName("last_updated")]        long?   LastUpdated,
    [property: JsonPropertyName("user_id")]             string? UserId
);

public record LbTopReleasesResponse(
    [property: JsonPropertyName("payload")] LbTopReleasesPayload Payload
);

#endregion

#region Top Release Groups

public record LbReleaseGroupStat(
    [property: JsonPropertyName("release_group_name")]  string  ReleaseGroupName,
    [property: JsonPropertyName("release_group_mbid")]  string? ReleaseGroupMbid,
    [property: JsonPropertyName("artist_name")]         string? ArtistName,
    [property: JsonPropertyName("artist_mbids")]        IReadOnlyList<string>?       ArtistMbids,
    [property: JsonPropertyName("artists")]             IReadOnlyList<LbArtistCredit>? Artists,
    [property: JsonPropertyName("listen_count")]        int     ListenCount,
    [property: JsonPropertyName("caa_id")]              long?   CaaId,
    [property: JsonPropertyName("caa_release_mbid")]    string? CaaReleaseMbid
)
{
    /// <summary>Cover Art Archive URL (500px), or null if no cover art available.</summary>
    public string? CoverArtUrl => CaaReleaseMbid is not null
        ? $"https://coverartarchive.org/release/{CaaReleaseMbid}/front-500"
        : null;
}

public record LbTopReleaseGroupsPayload(
    [property: JsonPropertyName("release_groups")]             IReadOnlyList<LbReleaseGroupStat> ReleaseGroups,
    [property: JsonPropertyName("count")]                      int     Count,
    [property: JsonPropertyName("total_release_group_count")]  int     TotalReleaseGroupCount,
    [property: JsonPropertyName("offset")]                     int     Offset,
    [property: JsonPropertyName("range")]                      string? Range,
    [property: JsonPropertyName("from_ts")]                    long?   FromTs,
    [property: JsonPropertyName("to_ts")]                      long?   ToTs,
    [property: JsonPropertyName("last_updated")]               long?   LastUpdated,
    [property: JsonPropertyName("user_id")]                    string? UserId
);

public record LbTopReleaseGroupsResponse(
    [property: JsonPropertyName("payload")] LbTopReleaseGroupsPayload Payload
);

#endregion

#region Top Recordings

public record LbRecordingStat(
    [property: JsonPropertyName("track_name")]       string  TrackName,
    [property: JsonPropertyName("recording_mbid")]   string? RecordingMbid,
    [property: JsonPropertyName("artist_name")]      string? ArtistName,
    [property: JsonPropertyName("artist_mbids")]     IReadOnlyList<string>?        ArtistMbids,
    [property: JsonPropertyName("artists")]          IReadOnlyList<LbArtistCredit>? Artists,
    [property: JsonPropertyName("release_name")]     string? ReleaseName,
    [property: JsonPropertyName("release_mbid")]     string? ReleaseMbid,
    [property: JsonPropertyName("listen_count")]     int     ListenCount,
    [property: JsonPropertyName("caa_id")]           long?   CaaId,
    [property: JsonPropertyName("caa_release_mbid")] string? CaaReleaseMbid
)
{
    /// <summary>Cover Art Archive URL (500px), or null if no cover art available.</summary>
    public string? CoverArtUrl => CaaReleaseMbid is not null
        ? $"https://coverartarchive.org/release/{CaaReleaseMbid}/front-500"
        : null;
}

public record LbTopRecordingsPayload(
    [property: JsonPropertyName("recordings")]             IReadOnlyList<LbRecordingStat> Recordings,
    [property: JsonPropertyName("count")]                  int     Count,
    [property: JsonPropertyName("total_recording_count")]  int     TotalRecordingCount,
    [property: JsonPropertyName("offset")]                 int     Offset,
    [property: JsonPropertyName("range")]                  string? Range,
    [property: JsonPropertyName("from_ts")]                long?   FromTs,
    [property: JsonPropertyName("to_ts")]                  long?   ToTs,
    [property: JsonPropertyName("last_updated")]           long?   LastUpdated,
    [property: JsonPropertyName("user_id")]                string? UserId
);

public record LbTopRecordingsResponse(
    [property: JsonPropertyName("payload")] LbTopRecordingsPayload Payload
);

#endregion

#region Listening Activity

public record LbListeningActivityEntry(
    [property: JsonPropertyName("listen_count")] int     ListenCount,
    [property: JsonPropertyName("time_range")]   string? TimeRange,
    [property: JsonPropertyName("from_ts")]      long?   FromTs,
    [property: JsonPropertyName("to_ts")]        long?   ToTs
);

public record LbListeningActivityPayload(
    [property: JsonPropertyName("listening_activity")] IReadOnlyList<LbListeningActivityEntry> ListeningActivity,
    [property: JsonPropertyName("range")]              string? Range,
    [property: JsonPropertyName("from_ts")]            long?   FromTs,
    [property: JsonPropertyName("to_ts")]              long?   ToTs,
    [property: JsonPropertyName("last_updated")]       long?   LastUpdated,
    [property: JsonPropertyName("user_id")]            string? UserId
);

public record LbListeningActivityResponse(
    [property: JsonPropertyName("payload")] LbListeningActivityPayload Payload
);

#endregion

#region Daily Activity

public record LbDailyActivityEntry(
    [property: JsonPropertyName("hour")]        int Hour,
    [property: JsonPropertyName("listen_count")] int ListenCount
);

public record LbDailyActivityPayload(
    [property: JsonPropertyName("daily_activity")] Dictionary<string, IReadOnlyList<LbDailyActivityEntry>> DailyActivity,
    [property: JsonPropertyName("range")]          string? Range,
    [property: JsonPropertyName("from_ts")]        long?   FromTs,
    [property: JsonPropertyName("to_ts")]          long?   ToTs,
    [property: JsonPropertyName("last_updated")]   long?   LastUpdated,
    [property: JsonPropertyName("user_id")]        string? UserId
);

public record LbDailyActivityResponse(
    [property: JsonPropertyName("payload")] LbDailyActivityPayload Payload
);

#endregion

#region Artist Map

public record LbArtistMapEntry(
    [property: JsonPropertyName("country")]      string Country,
    [property: JsonPropertyName("artist_count")] int    ArtistCount,
    [property: JsonPropertyName("listen_count")] int    ListenCount,
    [property: JsonPropertyName("artists")]      IReadOnlyList<LbArtistStat> Artists
);

public record LbArtistMapPayload(
    [property: JsonPropertyName("artist_map")]    IReadOnlyList<LbArtistMapEntry> ArtistMap,
    [property: JsonPropertyName("stats_range")]   string? StatsRange,
    [property: JsonPropertyName("from_ts")]       long?   FromTs,
    [property: JsonPropertyName("to_ts")]         long?   ToTs,
    [property: JsonPropertyName("last_updated")]  long?   LastUpdated
);

public record LbArtistMapResponse(
    [property: JsonPropertyName("payload")] LbArtistMapPayload Payload
);

#endregion

#region Listener Stats

public record LbUserListenStat(
    [property: JsonPropertyName("user_name")]   string UserName,
    [property: JsonPropertyName("listen_count")] int   ListenCount
);

public record LbArtistListenersPayload(
    [property: JsonPropertyName("artist_mbid")]      string ArtistMbid,
    [property: JsonPropertyName("artist_name")]      string ArtistName,
    [property: JsonPropertyName("listeners")]        IReadOnlyList<LbUserListenStat> Listeners,
    [property: JsonPropertyName("total_listen_count")] int TotalListenCount,
    [property: JsonPropertyName("total_user_count")]   int TotalUserCount,
    [property: JsonPropertyName("stats_range")]      string? StatsRange,
    [property: JsonPropertyName("from_ts")]          long?   FromTs,
    [property: JsonPropertyName("to_ts")]            long?   ToTs,
    [property: JsonPropertyName("last_updated")]     long?   LastUpdated
);

public record LbArtistListenersResponse(
    [property: JsonPropertyName("payload")] LbArtistListenersPayload Payload
);

public record LbReleaseGroupListenersPayload(
    [property: JsonPropertyName("release_group_mbid")] string ReleaseGroupMbid,
    [property: JsonPropertyName("release_group_name")] string? ReleaseGroupName,
    [property: JsonPropertyName("artist_name")]        string? ArtistName,
    [property: JsonPropertyName("artist_mbids")]       IReadOnlyList<string>? ArtistMbids,
    [property: JsonPropertyName("listeners")]          IReadOnlyList<LbUserListenStat> Listeners,
    [property: JsonPropertyName("total_listen_count")] int TotalListenCount,
    [property: JsonPropertyName("total_user_count")]   int TotalUserCount,
    [property: JsonPropertyName("stats_range")]        string? StatsRange,
    [property: JsonPropertyName("from_ts")]            long?   FromTs,
    [property: JsonPropertyName("to_ts")]              long?   ToTs,
    [property: JsonPropertyName("last_updated")]       long?   LastUpdated,
    [property: JsonPropertyName("caa_release_mbid")]   string? CaaReleaseMbid,
    [property: JsonPropertyName("caa_id")]             long?   CaaId
);

public record LbReleaseGroupListenersResponse(
    [property: JsonPropertyName("payload")] LbReleaseGroupListenersPayload Payload
);

#endregion

#region Popularity: Top Recordings for Artist

public record LbTopRecordingForArtist(
    [property: JsonPropertyName("recording_name")]   string  RecordingName,
    [property: JsonPropertyName("recording_mbid")]   string? RecordingMbid,
    [property: JsonPropertyName("total_listen_count")] long  TotalListenCount,
    [property: JsonPropertyName("total_user_count")]   long  TotalUserCount,
    [property: JsonPropertyName("artist_mbids")]     IReadOnlyList<string>? ArtistMbids,
    [property: JsonPropertyName("release_color")]    LbReleaseColor?        ReleaseColor,
    [property: JsonPropertyName("release_name")]     string? ReleaseName,
    [property: JsonPropertyName("release_mbid")]     string? ReleaseMbid,
    [property: JsonPropertyName("caa_release_mbid")] string? CaaReleaseMbid,
    [property: JsonPropertyName("caa_id")]           long?   CaaId
)
{
    public string? CoverArtUrl => CaaReleaseMbid is not null
        ? $"https://coverartarchive.org/release/{CaaReleaseMbid}/front-500"
        : null;
}

public record LbReleaseColor(
    [property: JsonPropertyName("red")]   int Red,
    [property: JsonPropertyName("green")] int Green,
    [property: JsonPropertyName("blue")]  int Blue,
    [property: JsonPropertyName("alpha")] float Alpha
);

#endregion

#region Popularity: Top Release Groups for Artist

public record LbTopReleaseGroupForArtist(
    [property: JsonPropertyName("release_group_name")]  string  ReleaseGroupName,
    [property: JsonPropertyName("release_group_mbid")]  string? ReleaseGroupMbid,
    [property: JsonPropertyName("total_listen_count")]  long    TotalListenCount,
    [property: JsonPropertyName("total_user_count")]    long    TotalUserCount,
    [property: JsonPropertyName("artist_mbids")]        IReadOnlyList<string>? ArtistMbids,
    [property: JsonPropertyName("caa_release_mbid")]    string? CaaReleaseMbid,
    [property: JsonPropertyName("caa_id")]              long?   CaaId
)
{
    public string? CoverArtUrl => CaaReleaseMbid is not null
        ? $"https://coverartarchive.org/release/{CaaReleaseMbid}/front-500"
        : null;
}

#endregion

#region Similar Users

public record LbSimilarUser(
    [property: JsonPropertyName("user_name")]   string  UserName,
    [property: JsonPropertyName("similarity")]  double  Similarity
);

public record LbSimilarUsersResponse(
    [property: JsonPropertyName("payload")] IReadOnlyList<LbSimilarUser> Payload
);

#endregion
