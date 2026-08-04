using System.Text.Json.Serialization;
using mashin.Converters;

namespace mashin.Models;

public sealed class FavoritesSnapshot
{
    [JsonPropertyName("tracks")]
    public List<FavoriteTrackSnapshot> Tracks { get; set; } = new();

    [JsonPropertyName("albums")]
    public List<FavoriteAlbumSnapshot> Albums { get; set; } = new();

    [JsonPropertyName("playlists")]
    public List<FavoritePlaylistSnapshot> Playlists { get; set; } = new();

    [JsonPropertyName("artists")]
    public List<FavoriteArtistSnapshot> Artists { get; set; } = new();
}

public sealed class FavoriteTrackSnapshot
{
    [JsonPropertyName("uri")]
    public string Uri { get; set; } = string.Empty;

    [JsonPropertyName("item_id")]
    public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("display_name")]
    public string? DisplayName { get; set; }

    [JsonPropertyName("duration")]
    public int Duration { get; set; }

    [JsonPropertyName("image_url")]
    public string? ImagePath { get; set; }

    [JsonPropertyName("album")]
    public FavoriteAlbumRef? Album { get; set; }

    [JsonPropertyName("artists")]
    public List<FavoriteArtistRef> Artists { get; set; } = new();

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class FavoriteAlbumSnapshot
{
    [JsonPropertyName("uri")]
    public string Uri { get; set; } = string.Empty;

    [JsonPropertyName("item_id")]
    public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("display_name")]
    public string? DisplayName { get; set; }

    [JsonPropertyName("year")]
    [JsonConverter(typeof(FlexibleIntConverter))]
    public int? Year { get; set; }

    [JsonPropertyName("image_url")]
    public string? ImagePath { get; set; }

    [JsonPropertyName("artists")]
    public List<FavoriteArtistRef> Artists { get; set; } = new();

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class FavoritePlaylistSnapshot
{
    [JsonPropertyName("uri")]
    public string Uri { get; set; } = string.Empty;

    [JsonPropertyName("item_id")]
    public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("display_name")]
    public string? DisplayName { get; set; }

    [JsonPropertyName("image_url")]
    public string? ImagePath { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class FavoriteArtistSnapshot
{
    [JsonPropertyName("uri")]
    public string Uri { get; set; } = string.Empty;

    [JsonPropertyName("item_id")]
    public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("display_name")]
    public string? DisplayName { get; set; }

    [JsonPropertyName("image_url")]
    public string? ImagePath { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class FavoriteAlbumRef
{
    [JsonPropertyName("item_id")]
    public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("year")]
    [JsonConverter(typeof(FlexibleIntConverter))]
    public int? Year { get; set; }

    [JsonPropertyName("image_url")]
    public string? ImagePath { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class FavoriteArtistRef
{
    [JsonPropertyName("item_id")]
    public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}
