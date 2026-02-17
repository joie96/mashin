using Sendspin.SDK.Client;
using Sendspin.SDK.Models;

namespace mashin.Services;

public class SettingsService
{
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
                              ?? GetDefaultAudioFormats();
            }
            catch
            {
                AudioFormats = GetDefaultAudioFormats();
            }
        }
        else
        {
            AudioFormats = GetDefaultAudioFormats();
        }
    }

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
        return new ClientCapabilities
        {
            ClientName = $"Mashin ({GetClientName()})",
            ClientId = $"mashin-{GetClientName().Replace(" ", string.Empty).ToLowerInvariant()}",
            BufferCapacity = BufferCapacity,
            AudioFormats = AudioFormats
        };
    }

    private static string GetClientName()
    {
#if ANDROID || IOS
        return Microsoft.Maui.Devices.DeviceInfo.Name;
#else
        return Environment.MachineName;
#endif
    }

    private static List<AudioFormat> GetDefaultAudioFormats() => new()
    {
        new AudioFormat { Codec = "opus", SampleRate = 48000, Channels = 2, Bitrate = 256 },
        new AudioFormat { Codec = "pcm", SampleRate = 48000, Channels = 2, BitDepth = 16 },
        new AudioFormat { Codec = "flac", SampleRate = 48000, Channels = 2 }
    };
}