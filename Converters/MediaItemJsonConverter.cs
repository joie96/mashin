using mashin.Models;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace mashin.Converters;

public sealed class MediaItemJsonConverter : JsonConverter<MediaItem>
{
    public override MediaItem? Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        using var document = JsonDocument.ParseValue(ref reader);
        var root = document.RootElement;

        if (root.ValueKind != JsonValueKind.Object)
        {
            return null;
        }

        if (!root.TryGetProperty("media_type", out var mediaTypeProperty))
        {
            return null;
        }

        var mediaType = mediaTypeProperty.GetString()?.ToLowerInvariant();
        var json = root.GetRawText();
        var nestedOptions = CreateOptionsWithoutSelf(options);

        return mediaType switch
        {
            "artist" => JsonSerializer.Deserialize<Artist>(json, nestedOptions),
            "album" => JsonSerializer.Deserialize<Album>(json, nestedOptions),
            "track" => JsonSerializer.Deserialize<Track>(json, nestedOptions),
            "playlist" => JsonSerializer.Deserialize<Playlist>(json, nestedOptions),
            "radio" => JsonSerializer.Deserialize<Radio>(json, nestedOptions),
            "podcast" => JsonSerializer.Deserialize<Podcast>(json, nestedOptions),
            "podcastepisode" => JsonSerializer.Deserialize<PodcastEpisode>(json, nestedOptions),
            "podcast_episode" => JsonSerializer.Deserialize<PodcastEpisode>(json, nestedOptions),
            "audiobook" => JsonSerializer.Deserialize<Audiobook>(json, nestedOptions),
            "folder" => JsonSerializer.Deserialize<FolderItem>(json, nestedOptions),
            "announcement" => JsonSerializer.Deserialize<Announcement>(json, nestedOptions),
            "flow_stream" => JsonSerializer.Deserialize<FlowStream>(json, nestedOptions),
            "flowstream" => JsonSerializer.Deserialize<FlowStream>(json, nestedOptions),
            "plugin_source" => JsonSerializer.Deserialize<PluginSource>(json, nestedOptions),
            "pluginsource" => JsonSerializer.Deserialize<PluginSource>(json, nestedOptions),
            "sound_effect" => JsonSerializer.Deserialize<SoundEffect>(json, nestedOptions),
            "soundeffect" => JsonSerializer.Deserialize<SoundEffect>(json, nestedOptions),
            "genre" => JsonSerializer.Deserialize<Genre>(json, nestedOptions),
            "unknown" => JsonSerializer.Deserialize<UnknownMediaItem>(json, nestedOptions),
            _ => null
        };
    }

    public override void Write(Utf8JsonWriter writer, MediaItem value, JsonSerializerOptions options)
    {
        var nestedOptions = CreateOptionsWithoutSelf(options);
        JsonSerializer.Serialize(writer, value, value.GetType(), nestedOptions);
    }

    private static JsonSerializerOptions CreateOptionsWithoutSelf(JsonSerializerOptions options)
    {
        var nestedOptions = new JsonSerializerOptions(options);

        for (var i = 0; i < nestedOptions.Converters.Count; i++)
        {
            if (nestedOptions.Converters[i] is MediaItemJsonConverter)
            {
                nestedOptions.Converters.RemoveAt(i);
                break;
            }
        }

        return nestedOptions;
    }
}