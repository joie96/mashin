using System.Text.Json;
using System.Text.Json.Serialization;

namespace mashin.Models;

public sealed record MusicAssistantQueueEvent(
    string Event,
    string? QueueId,
    PlayerQueue? Queue,
    double? ElapsedTimeSeconds,
    MusicAssistantQueueSettings? QueueSettings,
    Dictionary<string, JsonElement>? AdditionalData,
    DateTimeOffset ReceivedAt);

public sealed record MusicAssistantPlayerEvent(
    string Event,
    string? PlayerId,
    Player? Player,
    MusicAssistantPlayerSettings? PlayerSettings,
    MusicAssistantPlayerConfig? PlayerConfig,
    MusicAssistantPlayerDspConfig? PlayerDspConfig,
    MusicAssistantPlayerOptions? PlayerOptions,
    Dictionary<string, JsonElement>? AdditionalData,
    DateTimeOffset ReceivedAt);

public sealed class MusicAssistantQueueSettings
{
    [JsonPropertyName("shuffle_enabled")]
    public bool? ShuffleEnabled { get; set; }

    [JsonPropertyName("repeat_mode")]
    public RepeatMode? RepeatMode { get; set; }

    [JsonPropertyName("dont_stop_the_music_enabled")]
    public bool? DontStopTheMusicEnabled { get; set; }

    [JsonPropertyName("crossfade_enabled")]
    public bool? CrossfadeEnabled { get; set; }

    [JsonPropertyName("smart_fades_active")]
    public bool? SmartFadesActive { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantPlayerSettings
{
    [JsonPropertyName("values")]
    public Dictionary<string, MusicAssistantConfigEntry>? Values { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantPlayerConfig
{
    [JsonPropertyName("player_id")]
    public string? PlayerId { get; set; }

    [JsonPropertyName("provider")]
    public string? Provider { get; set; }

    [JsonPropertyName("enabled")]
    public bool? Enabled { get; set; }

    [JsonPropertyName("name")]
    public string? Name { get; set; }

    [JsonPropertyName("default_name")]
    public string? DefaultName { get; set; }

    [JsonPropertyName("values")]
    public Dictionary<string, MusicAssistantConfigEntry>? Values { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantPlayerDspConfig
{
    [JsonPropertyName("enabled")]
    public bool? Enabled { get; set; }

    [JsonPropertyName("input_gain")]
    public double? InputGain { get; set; }

    [JsonPropertyName("output_gain")]
    public double? OutputGain { get; set; }

    [JsonPropertyName("filters")]
    public List<MusicAssistantDspFilter>? Filters { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantDspFilter
{
    [JsonPropertyName("type")]
    public string? Type { get; set; }

    [JsonPropertyName("enabled")]
    public bool? Enabled { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantPlayerOptions
{
    public List<MusicAssistantPlayerOption>? PreviousOptions { get; set; }

    public List<MusicAssistantPlayerOption>? CurrentOptions { get; set; }
}

public sealed class MusicAssistantPlayerOption
{
    [JsonPropertyName("key")]
    public string? Key { get; set; }

    [JsonPropertyName("name")]
    public string? Name { get; set; }

    [JsonPropertyName("type")]
    public string? Type { get; set; }

    [JsonPropertyName("translation_key")]
    public string? TranslationKey { get; set; }

    [JsonPropertyName("read_only")]
    public bool? ReadOnly { get; set; }

    [JsonPropertyName("min_value")]
    public double? MinValue { get; set; }

    [JsonPropertyName("max_value")]
    public double? MaxValue { get; set; }

    [JsonPropertyName("step")]
    public double? Step { get; set; }

    [JsonPropertyName("value")]
    public JsonElement Value { get; set; }

    [JsonPropertyName("options")]
    public List<MusicAssistantPlayerOptionEntry>? Options { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantPlayerOptionEntry
{
    [JsonPropertyName("key")]
    public string? Key { get; set; }

    [JsonPropertyName("name")]
    public string? Name { get; set; }

    [JsonPropertyName("type")]
    public string? Type { get; set; }

    [JsonPropertyName("value")]
    public JsonElement Value { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantConfigEntry
{
    [JsonPropertyName("type")]
    public string? Type { get; set; }

    [JsonPropertyName("label")]
    public string? Label { get; set; }

    [JsonPropertyName("default_value")]
    public JsonElement DefaultValue { get; set; }

    [JsonPropertyName("required")]
    public bool? Required { get; set; }

    [JsonPropertyName("value")]
    public JsonElement Value { get; set; }

    [JsonPropertyName("read_only")]
    public bool? ReadOnly { get; set; }

    [JsonPropertyName("hidden")]
    public bool? Hidden { get; set; }

    [JsonPropertyName("category")]
    public string? Category { get; set; }

    [JsonPropertyName("action")]
    public string? Action { get; set; }

    [JsonPropertyName("action_label")]
    public string? ActionLabel { get; set; }

    [JsonPropertyName("requires_reload")]
    public bool? RequiresReload { get; set; }

    [JsonPropertyName("immediate_apply")]
    public bool? ImmediateApply { get; set; }

    [JsonPropertyName("depends_on")]
    public string? DependsOn { get; set; }

    [JsonPropertyName("depends_on_value")]
    public JsonElement DependsOnValue { get; set; }

    [JsonPropertyName("depends_on_value_not")]
    public JsonElement DependsOnValueNot { get; set; }

    [JsonPropertyName("options")]
    public List<MusicAssistantConfigValueOption>? Options { get; set; }

    [JsonPropertyName("range")]
    public List<double>? Range { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}

public sealed class MusicAssistantConfigValueOption
{
    [JsonPropertyName("title")]
    public string? Title { get; set; }

    [JsonPropertyName("value")]
    public JsonElement Value { get; set; }

    [JsonPropertyName("disabled")]
    public bool? Disabled { get; set; }

    [JsonPropertyName("disabled_reason")]
    public string? DisabledReason { get; set; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalData { get; set; }
}
