using Sendspin.SDK.Client;
using Sendspin.SDK.Models;

namespace mashin.Services;

public class SettingsService
{
    #region Constants

    // Central default values
    private const string DefaultMusicAssistantUrl = "http://192.168.1.2:8095";
    private const string DefaultSendspinUrl = "ws://192.168.1.2:8927/sendspin";
    private const int DefaultBufferCapacity = 64_000_000;

    // Preferences keys
    private const string MusicAssistantUrlKey = "music_assistant_url";
    private const string SendspinUrlKey = "sendspin_url";
    private const string AuthTokenKey = "auth_token";
    private const string UsernameKey = "username";
    private const string ThemeModeKey = "theme_mode";
    private const string BufferCapacityKey = "buffer_capacity";
    private const string AudioFormatsKey = "audio_formats"; // JSON serialized

    #endregion

    #region Properties

    // Server settings
    public string MusicAssistantUrl { get; set; }
    public string SendspinUrl { get; set; }

    // Authentication
    public string? AuthToken { get; set; }
    public string? Username { get; set; }

    // Theme setting (uses MAUI's AppTheme)
    public AppTheme ThemeMode { get; set; }

    // Audio/streaming settings
    public int BufferCapacity { get; set; }
    public List<AudioFormat> AudioFormats { get; set; }

    #endregion

    #region Construction

    public SettingsService()
    {
        // Load all settings
        MusicAssistantUrl = Preferences.Get(MusicAssistantUrlKey, DefaultMusicAssistantUrl);
        SendspinUrl = Preferences.Get(SendspinUrlKey, DefaultSendspinUrl);
        AuthToken = Preferences.Get(AuthTokenKey, (string?)null);
        Username = Preferences.Get(UsernameKey, (string?)null);

        // Load theme (0=Unspecified/System, 1=Light, 2=Dark)
        var themeInt = Preferences.Get(ThemeModeKey, (int)AppTheme.Dark);
        ThemeMode = (AppTheme)themeInt;

        // Load buffer capacity
        BufferCapacity = Preferences.Get(BufferCapacityKey, DefaultBufferCapacity);

        // Load audio formats (as JSON)
        var formatsJson = Preferences.Get(AudioFormatsKey, string.Empty);
        if (!string.IsNullOrEmpty(formatsJson))
        {
            try
            {
                AudioFormats = System.Text.Json.JsonSerializer.Deserialize<List<AudioFormat>>(formatsJson)
                              ?? BuildPreferredAudioFormats("opus")!;
            }
            catch
            {
                AudioFormats = BuildPreferredAudioFormats("opus")!;
            }
        }
        else
        {
            AudioFormats = BuildPreferredAudioFormats("opus")!;
        }

        // Keep a single preferred codec persisted in settings.
        var normalizedCodec = AudioFormats.FirstOrDefault()?.Codec;
        AudioFormats = BuildPreferredAudioFormats(normalizedCodec ?? "opus") ?? BuildPreferredAudioFormats("opus")!;
    }

    #endregion

    #region Public Methods

    public void Save()
    {
        // Server
        Preferences.Set(MusicAssistantUrlKey, MusicAssistantUrl);
        Preferences.Set(SendspinUrlKey, SendspinUrl);

        // Auth
        if (!string.IsNullOrEmpty(AuthToken))
            Preferences.Set(AuthTokenKey, AuthToken);
        else
            Preferences.Remove(AuthTokenKey);

        if (!string.IsNullOrEmpty(Username))
            Preferences.Set(UsernameKey, Username);
        else
            Preferences.Remove(UsernameKey);

        // Theme
        Preferences.Set(ThemeModeKey, (int)ThemeMode);

        // Buffer capacity
        Preferences.Set(BufferCapacityKey, BufferCapacity);

        // Save audio formats as JSON
        var formatsJson = System.Text.Json.JsonSerializer.Serialize(AudioFormats);
        Preferences.Set(AudioFormatsKey, formatsJson);
    }

    public ClientCapabilities GetClientCapabilities()
    {
        var clientName = GetClientName();

        return new ClientCapabilities
        {
            ClientName = $"Mashin ({clientName})",
            ClientId = $"mashin-{clientName.Replace(" ", string.Empty).ToLowerInvariant()}",
            BufferCapacity = BufferCapacity,
            AudioFormats = AudioFormats
        };
    }

    public string GetPreferredAudioCodec()
    {
        var codec = AudioFormats.FirstOrDefault()?.Codec?.Trim().ToLowerInvariant();
        return codec switch
        {
            "opus" => "opus",
            "flac" => "flac",
            "pcm" => "pcm",
            _ => "opus",
        };
    }

    public bool SetPreferredAudioCodec(string codec)
    {
        var normalizedCodec = codec?.Trim().ToLowerInvariant();
        if (string.IsNullOrWhiteSpace(normalizedCodec))
        {
            return false;
        }

        var preferredFormats = BuildPreferredAudioFormats(normalizedCodec);
        if (preferredFormats == null)
        {
            return false;
        }

        var changed = AudioFormats.Count != preferredFormats.Count
            || AudioFormats.Count == 0
            || !string.Equals(AudioFormats[0].Codec, preferredFormats[0].Codec, StringComparison.OrdinalIgnoreCase);
        AudioFormats = preferredFormats;
        Save();
        return changed;
    }

    #endregion

    #region Helpers

    private static string GetClientName()
    {
#if ANDROID || IOS
        return Microsoft.Maui.Devices.DeviceInfo.Name;
#else
        return Environment.MachineName;
#endif
    }

    private static List<AudioFormat>? BuildPreferredAudioFormats(string codec)
    {
        var normalizedCodec = codec.Trim().ToLowerInvariant();
        var preferred = normalizedCodec switch
        {
            "opus" => new AudioFormat { Codec = "opus", SampleRate = 48000, Channels = 2, Bitrate = 256 },
            "pcm" => new AudioFormat { Codec = "pcm", SampleRate = 48000, Channels = 2, BitDepth = 16 },
            "flac" => new AudioFormat { Codec = "flac", SampleRate = 48000, Channels = 2 },
            _ => null,
        };

        if (preferred == null)
        {
            return null;
        }

        return new List<AudioFormat> { preferred };
    }

    #endregion
}