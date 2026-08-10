using mashin.Converters;
using System.Text.Json.Serialization;

namespace mashin.Models;

public sealed class ArtistRef
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

public sealed class AlbumRef
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

    [JsonPropertyName("image_proxy_id")]
    public string? ImageProxyId { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class TrackSnapshot
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

    [JsonPropertyName("image_proxy_id")]
    public string? ImageProxyId { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();

    [JsonPropertyName("duration")]
    public int Duration { get; set; }

    [JsonPropertyName("index")]
    public int? Index { get; set; }

    [JsonPropertyName("album")]
    public AlbumRef? Album { get; set; }

    [JsonPropertyName("artists")]
    public List<ArtistRef> Artists { get; set; } = new();
}

public sealed class AlbumSnapshot
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

    [JsonPropertyName("image_proxy_id")]
    public string? ImageProxyId { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();

    [JsonPropertyName("year")]
    [JsonConverter(typeof(FlexibleIntConverter))]
    public int? Year { get; set; }

    [JsonPropertyName("artists")]
    public List<ArtistRef> Artists { get; set; } = new();
}

public sealed class PlaylistSnapshot
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

    [JsonPropertyName("image_proxy_id")]
    public string? ImageProxyId { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();

    [JsonPropertyName("owner")]
    public string? Owner { get; set; }

    [JsonPropertyName("is_editable")]
    public bool IsEditable { get; set; }

    [JsonPropertyName("sort_name")]
    public string? SortName { get; set; }

    [JsonPropertyName("items")]
    public List<TrackSnapshot> Items { get; set; } = new();
}

public sealed class ArtistSnapshot
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

    [JsonPropertyName("image_proxy_id")]
    public string? ImageProxyId { get; set; }

    [JsonPropertyName("provider_mappings")]
    public List<ProviderMapping> ProviderMappings { get; set; } = new();
}

public sealed class FavoritesSnapshot
{
    [JsonPropertyName("tracks")]
    public List<TrackSnapshot> Tracks { get; set; } = new();

    [JsonPropertyName("albums")]
    public List<AlbumSnapshot> Albums { get; set; } = new();

    [JsonPropertyName("playlists")]
    public List<PlaylistSnapshot> Playlists { get; set; } = new();

    [JsonPropertyName("artists")]
    public List<ArtistSnapshot> Artists { get; set; } = new();
}

public sealed class PlaylistsSnapshot
{
    [JsonPropertyName("playlists")]
    public List<PlaylistSnapshot> Playlists { get; set; } = new();
}

public static class UserDataSnapshotMapper
{
    public static TrackSnapshot ToTrackSnapshot(Track track, int? indexOverride = null)
    {
        var snapshot = new TrackSnapshot
        {
            Uri = track.Uri ?? string.Empty,
            ItemId = track.ItemId,
            Provider = track.Provider,
            Name = track.Name,
            DisplayName = track.DisplayName,
            Duration = track.Duration,
            Index = indexOverride ?? track.Index,
            ImagePath = track.PrimaryImage?.Path,
            ImageProxyId = track.PrimaryImage?.ProxyId,
            ProviderMappings = CloneProviderMappings(track.ProviderMappings)
        };

        if (track.Album != null)
        {
            snapshot.Album = ToAlbumRef(track.Album);
        }

        if (track.Artists != null)
        {
            foreach (var artist in track.Artists)
            {
                snapshot.Artists.Add(ToArtistRef(artist));
            }
        }

        return snapshot;
    }

    public static AlbumSnapshot ToAlbumSnapshot(Album album)
    {
        var snapshot = new AlbumSnapshot
        {
            Uri = album.Uri ?? string.Empty,
            ItemId = album.ItemId,
            Provider = album.Provider,
            Name = album.Name,
            DisplayName = album.DisplayName,
            Year = album.Year,
            ImagePath = album.PrimaryImage?.Path,
            ImageProxyId = album.PrimaryImage?.ProxyId,
            ProviderMappings = CloneProviderMappings(album.ProviderMappings)
        };

        if (album.Artists != null)
        {
            foreach (var artist in album.Artists)
            {
                snapshot.Artists.Add(ToArtistRef(artist));
            }
        }

        return snapshot;
    }

    public static ArtistSnapshot ToArtistSnapshot(Artist artist)
    {
        return new ArtistSnapshot
        {
            Uri = artist.Uri ?? string.Empty,
            ItemId = artist.ItemId,
            Provider = artist.Provider,
            Name = artist.Name,
            DisplayName = artist.DisplayName,
            ImagePath = artist.PrimaryImage?.Path,
            ImageProxyId = artist.PrimaryImage?.ProxyId,
            ProviderMappings = CloneProviderMappings(artist.ProviderMappings)
        };
    }

    public static PlaylistSnapshot ToPlaylistSnapshot(Playlist playlist, bool includeItems = true)
    {
        var snapshot = new PlaylistSnapshot
        {
            Uri = playlist.Uri ?? string.Empty,
            ItemId = playlist.ItemId,
            Provider = playlist.Provider,
            Name = playlist.Name,
            DisplayName = playlist.DisplayName,
            SortName = playlist.SortName,
            Owner = playlist.Owner,
            IsEditable = playlist.IsEditable,
            ImagePath = playlist.PrimaryImage?.Path,
            ImageProxyId = playlist.PrimaryImage?.ProxyId,
            ProviderMappings = CloneProviderMappings(playlist.ProviderMappings)
        };

        if (includeItems)
        {
            snapshot.Items = playlist.Items
                .Select((track, index) => ToTrackSnapshot(track, index))
                .ToList();
        }

        return snapshot;
    }

    public static AlbumRef ToAlbumRef(Album album)
    {
        return new AlbumRef
        {
            ItemId = album.ItemId,
            Provider = album.Provider,
            Name = album.Name,
            Year = album.Year,
            ImagePath = album.PrimaryImage?.Path,
            ImageProxyId = album.PrimaryImage?.ProxyId,
            ProviderMappings = CloneProviderMappings(album.ProviderMappings)
        };
    }

    public static ArtistRef ToArtistRef(Artist artist)
    {
        return new ArtistRef
        {
            ItemId = artist.ItemId,
            Provider = artist.Provider,
            Name = artist.Name,
            ProviderMappings = CloneProviderMappings(artist.ProviderMappings)
        };
    }

    public static Track ToTrack(TrackSnapshot snapshot, bool favorite = true)
    {
        var track = new Track
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Duration = snapshot.Duration,
            Favorite = favorite,
            ProviderMappings = CloneProviderMappings(snapshot.ProviderMappings)
        };

        if (snapshot.Index.HasValue)
        {
            track.Index = Math.Max(0, snapshot.Index.Value);
        }

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            track.DisplayName = snapshot.DisplayName;
        }

        track.Metadata = BuildMetadata(snapshot.ImagePath, snapshot.ImageProxyId);

        if (snapshot.Album != null)
        {
            var album = new Album
            {
                ItemId = snapshot.Album.ItemId,
                Provider = snapshot.Album.Provider,
                Name = snapshot.Album.Name,
                Year = snapshot.Album.Year,
                Favorite = false,
                ProviderMappings = CloneProviderMappings(snapshot.Album.ProviderMappings)
            };

            album.DisplayName = snapshot.Album.Name;
            album.Metadata = BuildMetadata(snapshot.Album.ImagePath, snapshot.Album.ImageProxyId);
            track.Album = album;
        }

        if (snapshot.Artists.Count > 0)
        {
            track.Artists = snapshot.Artists
                .Select(ToArtist)
                .ToList();
        }

        return track;
    }

    public static Album ToAlbum(AlbumSnapshot snapshot, bool favorite = true)
    {
        var album = new Album
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Year = snapshot.Year,
            Favorite = favorite,
            ProviderMappings = CloneProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            album.DisplayName = snapshot.DisplayName;
        }

        album.Metadata = BuildMetadata(snapshot.ImagePath, snapshot.ImageProxyId);

        if (snapshot.Artists.Count > 0)
        {
            album.Artists = snapshot.Artists
                .Select(ToArtist)
                .ToList();
        }

        return album;
    }

    public static Playlist ToPlaylist(PlaylistSnapshot snapshot, bool favorite = false)
    {
        var playlist = new Playlist
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Favorite = favorite,
            SortName = snapshot.SortName,
            Owner = snapshot.Owner,
            IsEditable = snapshot.IsEditable,
            ProviderMappings = CloneProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            playlist.DisplayName = snapshot.DisplayName;
        }

        playlist.Metadata = BuildMetadata(snapshot.ImagePath, snapshot.ImageProxyId);

        var tracks = snapshot.Items
            .OrderBy(track => track.Index ?? int.MaxValue)
            .Select(item => ToTrack(item, favorite: false))
            .ToList();

        for (var i = 0; i < tracks.Count; i++)
        {
            tracks[i].Index = i;
        }

        playlist.Items = tracks;
        return playlist;
    }

    public static Artist ToArtist(ArtistSnapshot snapshot, bool favorite = true)
    {
        var artist = new Artist
        {
            Uri = snapshot.Uri,
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            Favorite = favorite,
            ProviderMappings = CloneProviderMappings(snapshot.ProviderMappings)
        };

        if (!string.IsNullOrWhiteSpace(snapshot.DisplayName))
        {
            artist.DisplayName = snapshot.DisplayName;
        }

        artist.Metadata = BuildMetadata(snapshot.ImagePath, snapshot.ImageProxyId);
        return artist;
    }

    public static Artist ToArtist(ArtistRef snapshot)
    {
        var artist = new Artist
        {
            ItemId = snapshot.ItemId,
            Provider = snapshot.Provider,
            Name = snapshot.Name,
            ProviderMappings = CloneProviderMappings(snapshot.ProviderMappings)
        };

        artist.DisplayName = snapshot.Name;
        return artist;
    }

    public static List<ProviderMapping> CloneProviderMappings(List<ProviderMapping>? mappings)
    {
        if (mappings == null || mappings.Count == 0)
        {
            return new List<ProviderMapping>();
        }

        return mappings
            .Select(CloneProviderMapping)
            .ToList();
    }

    public static ProviderMapping CloneProviderMapping(ProviderMapping mapping)
    {
        return new ProviderMapping
        {
            ItemId = mapping.ItemId,
            ProviderDomain = mapping.ProviderDomain,
            ProviderInstance = mapping.ProviderInstance,
            Available = mapping.Available,
            Url = mapping.Url
        };
    }

    public static MediaItemMetadata? BuildMetadata(string? imagePath, string? imageProxyId = null)
    {
        if (string.IsNullOrWhiteSpace(imagePath))
        {
            return null;
        }

        return new MediaItemMetadata
        {
            Images = new List<MediaItemImage>
            {
                new()
                {
                    Path = imagePath,
                    ProxyId = imageProxyId,
                    Provider = string.Empty,
                    RemotelyAccessible = true
                }
            }
        };
    }
}
