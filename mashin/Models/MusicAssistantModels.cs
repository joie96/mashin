using mashin.Converters;
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace mashin.Models
{
    /// <summary>
    /// Music Assistant API ApiResponse wrapper
    /// </summary>
    public class ApiResponse<T>
    {
        [JsonPropertyName("result")]
        public T? Result { get; set; }

        [JsonPropertyName("error")]
        public ApiError? Error { get; set; }

        public bool IsSuccess => Error == null;
    }

    /// <summary>
    /// Music Assistant API ApiError wrapper
    /// </summary>
    public class ApiError
    {
        [JsonPropertyName("code")]
        public string Code { get; set; } = string.Empty;

        [JsonPropertyName("message")]
        public string Message { get; set; } = string.Empty;
    }

    /// <summary>
    /// Base class for playable media items
    /// </summary>
    public abstract class MediaItem : INotifyPropertyChanged
    {
        private bool _isSelected;
        private bool _isPlaying;
        private bool _favorite;
        private ProviderManifest? _providerManifest;

        public event PropertyChangedEventHandler? PropertyChanged;

        [JsonPropertyName("item_id")]
        public string ItemId { get; set; } = string.Empty;

        [JsonPropertyName("provider")]
        public string Provider { get; set; } = string.Empty;

        [JsonIgnore]
        public string ProviderName
        {
            get
            {
                // Extract domain from provider instance (e.g., "deezer--WXaG8W2Y" -> "deezer")
                if (string.IsNullOrEmpty(Provider))
                    return string.Empty;

                var separatorIndex = Provider.IndexOf("--", StringComparison.Ordinal);
                return separatorIndex >= 0 ? Provider[..separatorIndex] : Provider;
            }
        }

        [JsonIgnore]
        public ProviderManifest? ProviderManifest
        {
            get => _providerManifest;
            set
            {
                if (_providerManifest == value)
                {
                    return;
                }

                _providerManifest = value;
                OnPropertyChanged();
            }
        }

        [JsonPropertyName("name")]
        public string Name { get; set; } = string.Empty;

        [JsonPropertyName("sort_name")]
        public string? SortName { get; set; }

        [JsonPropertyName("uri")]
        public string? Uri { get; set; }

        [JsonPropertyName("media_type")]
        public abstract MediaType MediaType { get; set; }

        [JsonPropertyName("provider_mappings")]
        public List<ProviderMapping> ProviderMappings { get; set; } = new();

        [JsonPropertyName("metadata")]
        public MediaItemMetadata? Metadata { get; set; }

        [JsonPropertyName("favorite")]
        public bool Favorite
        {
            get => _favorite;
            set
            {
                if (_favorite == value)
                {
                    return;
                }

                _favorite = value;
                OnPropertyChanged();
            }
        }

        [JsonPropertyName("external_ids")]
        public List<List<string>>? ExternalIds { get; set; }

        [JsonIgnore]
        public virtual string? ImageUrl => Metadata?.Images?.FirstOrDefault()?.Path;

        [JsonIgnore]
        public bool IsSelected
        {
            get => _isSelected;
            set
            {
                if (_isSelected == value)
                {
                    return;
                }

                _isSelected = value;
                OnPropertyChanged();
            }
        }

        [JsonIgnore]
        public bool IsPlaying
        {
            get => _isPlaying;
            set
            {
                if (_isPlaying == value)
                {
                    return;
                }

                _isPlaying = value;
                OnPropertyChanged();
            }
        }

        protected void OnPropertyChanged([CallerMemberName] string? propertyName = null)
            => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    /// <summary>
    /// Music Assistant API Track wrapper
    /// </summary>
    public class Track : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Track;

        [JsonPropertyName("duration")]
        public int Duration { get; set; }

        [JsonPropertyName("artists")]
        public List<Artist>? Artists { get; set; }

        [JsonPropertyName("album")]
        public Album? Album { get; set; }

        [JsonPropertyName("disc_number")]
        public int DiscNumber { get; set; }

        [JsonPropertyName("track_number")]
        public int TrackNumber { get; set; }

        [JsonIgnore]
        public int Index { get; set; }

        [JsonIgnore]
        public override string? ImageUrl => Album?.ImageUrl ?? Metadata?.Images?.FirstOrDefault()?.Path;

        [JsonIgnore]
        public string ArtistName => Artists?.FirstOrDefault()?.Name ?? "Unknown Artist";

        [JsonIgnore]
        public string AlbumName => Album?.Name ?? "Unknown Album";

        [JsonIgnore]
        public int ArtistsCount => Artists?.Count ?? 0;

        [JsonIgnore]
        public TimeSpan DurationTimeSpan => TimeSpan.FromSeconds(Duration);

    }

    /// <summary>
    /// Music Assistant API Album wrapper
    /// </summary>
    public class Album : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Album;

        [JsonPropertyName("year")]
        [JsonConverter(typeof(FlexibleIntConverter))]
        public int? Year { get; set; }

        [JsonPropertyName("album_type")]
        public string? AlbumType { get; set; }

        [JsonPropertyName("artists")]
        public List<Artist>? Artists { get; set; }

        [JsonIgnore]
        public string ArtistName => Artists?.FirstOrDefault()?.Name ?? "Unknown Artist";
    }

    /// <summary>
    /// Music Assistant API Artist wrapper
    /// </summary>
    public class Artist : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Artist;
    }

    /// <summary>
    /// Music Assistant API Playlist wrapper
    /// </summary>
    public class Playlist : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Playlist;

        [JsonPropertyName("owner")]
        public string? Owner { get; set; }

        [JsonPropertyName("is_editable")]
        public bool IsEditable { get; set; }
    }

    /// <summary>
    /// Music Assistant API Radio wrapper
    /// </summary>
    public class Radio : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Radio;
    }

    /// <summary>
    /// Music Assistant API Podcast wrapper
    /// </summary>
    public class Podcast : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Podcast;
    }

    /// <summary>
    /// Music Assistant API PodcastEpisode wrapper
    /// </summary>
    public class PodcastEpisode : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.PodcastEpisode;

        [JsonPropertyName("duration")]
        public int Duration { get; set; }

        [JsonPropertyName("podcast")]
        public Podcast? Podcast { get; set; }

        [JsonIgnore]
        public TimeSpan DurationTimeSpan => TimeSpan.FromSeconds(Duration);
    }

    /// <summary>
    /// Music Assistant API Audiobook wrapper
    /// </summary>
    public class Audiobook : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Audiobook;
    }

    /// <summary>
    /// Music Assistant API Genre wrapper
    /// </summary>
    public class Genre : MediaItem
    {
        public override MediaType MediaType { get; set; } = MediaType.Genre;
    }

    /// <summary>
    /// Music Assistant API BrowseFolder wrapper
    /// </summary>
    public class BrowseFolder
    {
        [JsonPropertyName("item_id")]
        public string ItemId { get; set; } = string.Empty;

        [JsonPropertyName("provider")]
        public string Provider { get; set; } = string.Empty;

        [JsonPropertyName("name")]
        public string Name { get; set; } = string.Empty;

        [JsonPropertyName("path")]
        public string Path { get; set; } = string.Empty;

        [JsonPropertyName("media_type")]
        public MediaType MediaType { get; set; } = MediaType.Folder;
    }

    /// <summary>
    /// Music Assistant API MediaItemMetadata wrapper
    /// </summary>
    public class MediaItemMetadata
    {
        [JsonPropertyName("description")]
        public string? Description { get; set; }

        [JsonPropertyName("images")]
        public List<MediaItemImage>? Images { get; set; }

        [JsonPropertyName("genres")]
        public List<string>? Genres { get; set; }

        [JsonPropertyName("label")]
        public string? Label { get; set; }

        [JsonPropertyName("popularity")]
        public int? Popularity { get; set; }

        [JsonPropertyName("release_date")]
        public DateTime? ReleaseDate { get; set; }
    }

    /// <summary>
    /// Music Assistant API MediaItemImage wrapper
    /// </summary>
    public class MediaItemImage
    {
        [JsonPropertyName("type")]
        public string Type { get; set; } = "thumb";

        [JsonPropertyName("path")]
        public string Path { get; set; } = string.Empty;

        [JsonPropertyName("provider")]
        public string Provider { get; set; } = string.Empty;

        [JsonPropertyName("remotely_accessible")]
        public bool RemotelyAccessible { get; set; }
    }

    /// <summary>
    /// Music Assistant API MediaType enum
    /// </summary>
    [JsonConverter(typeof(JsonStringEnumConverter))]
    public enum MediaType
    {
        Artist,
        Album,
        Track,
        Playlist,
        Radio,
        Audiobook,
        Podcast,
        PodcastEpisode,
        Folder,
        Genre,
        Unknown
    }

    /// <summary>
    /// Music Assistant API AlbumType enum
    /// </summary>
    [JsonConverter(typeof(JsonStringEnumConverter))]
    public enum AlbumType
    {
        Album,
        Single,
        Compilation,
        EP,
        Unknown
    }

    /// <summary>
    /// Music Assistant API ProviderMapping wrapper
    /// </summary>
    public class ProviderMapping
    {
        [JsonPropertyName("item_id")]
        public string ItemId { get; set; } = string.Empty;

        [JsonPropertyName("provider_domain")]
        public string ProviderDomain { get; set; } = string.Empty;

        [JsonPropertyName("provider_instance")]
        public string ProviderInstance { get; set; } = string.Empty;

        [JsonPropertyName("available")]
        public bool Available { get; set; } = true;

        [JsonPropertyName("url")]
        public string? Url { get; set; }
    }

    /// <summary>
    /// Music Assistant API AuthResponse wrapper
    /// </summary>
    public class AuthResponse
    {
        [JsonPropertyName("success")]
        public bool Success { get; set; }

        [JsonPropertyName("access_token")]
        public string? Token { get; set; }

        [JsonPropertyName("user")]
        public AuthUser? User { get; set; }
    }

    /// <summary>
    /// Music Assistant API AuthUser wrapper
    /// </summary>
    public class AuthUser
    {
        [JsonPropertyName("user_id")]
        public string? UserId { get; set; }

        [JsonPropertyName("username")]
        public string? Username { get; set; }

        [JsonPropertyName("display_name")]
        public string? DisplayName { get; set; }

        [JsonPropertyName("avatar_url")]
        public string? AvatarUrl { get; set; }

        [JsonPropertyName("preferences")]
        public Dictionary<string, object>? Preferences { get; set; }

        [JsonPropertyName("role")]
        public string? Role { get; set; }

        [JsonPropertyName("player_filter")]
        public List<string>? PlayerFilter { get; set; }

        [JsonPropertyName("provider_filter")]
        public List<string>? ProviderFilter { get; set; }
    }

    /// <summary>
    /// Music Assistant API SearchResults wrapper
    /// </summary>
    public class SearchResults
    {
        [JsonPropertyName("artists")]
        public List<Artist> Artists { get; set; } = new();

        [JsonPropertyName("albums")]
        public List<Album> Albums { get; set; } = new();

        [JsonPropertyName("tracks")]
        public List<Track> Tracks { get; set; } = new();

        [JsonPropertyName("playlists")]
        public List<Playlist> Playlists { get; set; } = new();

        [JsonPropertyName("radio")]
        public List<Radio> Radio { get; set; } = new();
    }

    /// <summary>
    /// Music Assistant API QueueOption enum
    /// </summary>
    [JsonConverter(typeof(JsonStringEnumConverter))]
    public enum QueueOption
    {
        Play,
        Replace,
        Next,
        ReplaceNext,
        Add
    }

    /// <summary>
    /// Music Assistant API RepeatMode enum
    /// </summary>
    [JsonConverter(typeof(JsonStringEnumConverter))]
    public enum RepeatMode
    {
        Off,
        One,
        All
    }

    /// <summary>
    /// Music Assistant API PlayerQueue wrapper
    /// </summary>
    public class PlayerQueue
    {
        [JsonPropertyName("queue_id")]
        public string QueueId { get; set; } = string.Empty;

        [JsonPropertyName("display_name")]
        public string DisplayName { get; set; } = string.Empty;

        [JsonPropertyName("active")]
        public bool Active { get; set; }

        [JsonPropertyName("items")]
        public int ItemCount { get; set; }

        [JsonPropertyName("state")]
        public string State { get; set; } = "idle";

        [JsonPropertyName("current_index")]
        public int? CurrentIndex { get; set; }

        [JsonPropertyName("elapsed_time")]
        public double? ElapsedTime { get; set; }

        [JsonPropertyName("current_item")]
        public QueueItem? CurrentItem { get; set; }

        [JsonPropertyName("next_item")]
        public QueueItem? NextItem { get; set; }

        [JsonPropertyName("shuffle_enabled")]
        public bool ShuffleEnabled { get; set; }

        [JsonPropertyName("repeat_mode")]
        public RepeatMode RepeatMode { get; set; }
    }

    /// <summary>
    /// Music Assistant API QueueItem wrapper
    /// </summary>
    public class QueueItem
    {
        [JsonPropertyName("queue_item_id")]
        public string QueueItemId { get; set; } = string.Empty;

        [JsonPropertyName("name")]
        public string Name { get; set; } = string.Empty;

        [JsonPropertyName("duration")]
        public int? Duration { get; set; }

        [JsonPropertyName("media_item")]
        public Track? MediaItem { get; set; }

        [JsonPropertyName("image")]
        public MediaItemImage? Image { get; set; }

        [JsonIgnore]
        public TimeSpan? DurationTimeSpan => Duration.HasValue ? TimeSpan.FromSeconds(Duration.Value) : null;
    }

    /// <summary>
    /// Music Assistant API Player wrapper
    /// </summary>
    public class Player
    {
        [JsonPropertyName("player_id")]
        public string PlayerId { get; set; } = string.Empty;

        [JsonPropertyName("name")]
        public string Name { get; set; } = string.Empty;

        [JsonPropertyName("available")]
        public bool Available { get; set; }

        [JsonPropertyName("volume_level")]
        public int? VolumeLevel { get; set; }

        [JsonPropertyName("volume_muted")]
        public bool? VolumeMuted { get; set; }

        [JsonPropertyName("state")]
        public string State { get; set; } = "idle";

        [JsonPropertyName("powered")]
        public bool? Powered { get; set; }

        [JsonPropertyName("provider")]
        public string Provider { get; set; } = string.Empty;

        [JsonPropertyName("type")]
        public string Type { get; set; } = "player";

        [JsonPropertyName("supported_features")]
        public List<string>? SupportedFeatures { get; set; }

        [JsonPropertyName("group_childs")]
        public List<string>? GroupChilds { get; set; }

        [JsonPropertyName("active_source")]
        public string? ActiveSource { get; set; }
    }

    /// <summary>
    /// Music Assistant API ServerInfoMessage wrapper
    /// </summary>
    public class ServerInfoMessage
    {
        [JsonPropertyName("server_id")]
        public string? ServerId { get; set; }

        [JsonPropertyName("server_version")]
        public string? ServerVersion { get; set; }

        [JsonPropertyName("schema_version")]
        public int SchemaVersion { get; set; }

        [JsonPropertyName("min_supported_schema_version")]
        public int MinSupportedSchemaVersion { get; set; }

        [JsonPropertyName("homeassistant_addon")]
        public bool HomeAssistantAddon { get; set; }
    }

    /// <summary>
    /// Music Assistant API ProviderManifest wrapper
    /// </summary>
    public class ProviderManifest
    {
        [JsonPropertyName("type")]
        public string Type { get; set; } = string.Empty;

        [JsonPropertyName("domain")]
        public string Domain { get; set; } = string.Empty;

        [JsonPropertyName("name")]
        public string Name { get; set; } = string.Empty;

        [JsonPropertyName("description")]
        public string Description { get; set; } = string.Empty;

        [JsonPropertyName("codeowners")]
        public List<string> CodeOwners { get; set; } = new();

        [JsonPropertyName("stage")]
        public string? Stage { get; set; }

        [JsonPropertyName("requirements")]
        public List<string>? Requirements { get; set; }

        [JsonPropertyName("documentation")]
        public string? Documentation { get; set; }

        [JsonPropertyName("multi_instance")]
        public bool MultiInstance { get; set; }

        [JsonPropertyName("builtin")]
        public bool Builtin { get; set; }

        [JsonPropertyName("allow_disable")]
        public bool AllowDisable { get; set; }

        [JsonPropertyName("depends_on")]
        public string? DependsOn { get; set; }

        [JsonPropertyName("icon")]
        public string? Icon { get; set; }

        [JsonPropertyName("icon_svg")]
        public string? IconSvg { get; set; }

        [JsonPropertyName("icon_svg_dark")]
        public string? IconSvgDark { get; set; }

        [JsonPropertyName("icon_svg_monochrome")]
        public string? IconSvgMonochrome { get; set; }

        [JsonPropertyName("mdns_discovery")]
        public List<string>? MdnsDiscovery { get; set; }

        [JsonPropertyName("credits")]
        public List<string>? Credits { get; set; }
    }
}