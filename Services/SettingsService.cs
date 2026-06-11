using Sendspin.SDK.Client;
using Sendspin.SDK.Models;

namespace mashin.Services;

public class SettingsService
{
    #region Constants

    // Central default values
    private const string DefaultMusicAssistantUrl = "http://192.168.1.2:8095";
    private const string DefaultSendspinUrl = "ws://192.168.1.2:8927/sendspin";
    private const int DefaultSendspinBufferCapacity = 64_000_000;

    // Preferences keys
    private const string MusicAssistantUrlKey = "music_assistant_url";
    private const string SendspinUrlKey = "sendspin_url";
    private const string AuthTokenKey = "auth_token";
    private const string UsernameKey = "username";
    private const string ThemeModeKey = "theme_mode";
    private const string SendspinBufferCapacityKey = "buffer_capacity";
    private const string SendspinAudioFormatsKey = "audio_formats"; // JSON serialized
    private const string InitialVolumeKey = "sendspin_initial_volume";
    private const string InitialMutedKey = "sendspin_initial_muted";
    private const int DefaultInitialVolume = 50;
    private const bool DefaultInitialMuted = false;

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

    // Sendspin streaming settings
    public int SendspinBufferCapacity { get; set; }
    public List<AudioFormat> SendspinAudioFormats { get; set; }
   
    public int InitialVolume { get; private set; }
    public bool InitialMuted { get; private set; }

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

        // Load Sendspin buffer capacity
        SendspinBufferCapacity = Preferences.Get(SendspinBufferCapacityKey, DefaultSendspinBufferCapacity);

        // Load Sendspin audio formats (as JSON)
        var sendspinFormatsJson = Preferences.Get(SendspinAudioFormatsKey, string.Empty);
        if (!string.IsNullOrEmpty(sendspinFormatsJson))
        {
            try
            {
            SendspinAudioFormats = System.Text.Json.JsonSerializer.Deserialize<List<AudioFormat>>(sendspinFormatsJson)
                                       ?? BuildSendspinPreferredAudioFormats("opus")!;
            }
            catch
            {
                SendspinAudioFormats = BuildSendspinPreferredAudioFormats("opus")!;
            }
        }
        else
        {
            SendspinAudioFormats = BuildSendspinPreferredAudioFormats("opus")!;
        }

        // Keep a single preferred codec persisted in settings.
        var normalizedCodec = SendspinAudioFormats.FirstOrDefault()?.Codec;
        SendspinAudioFormats = BuildSendspinPreferredAudioFormats(normalizedCodec ?? "opus")
            ?? BuildSendspinPreferredAudioFormats("opus")!;

        // Load initial player state for Sendspin handshake
        InitialVolume = Math.Clamp(Preferences.Get(InitialVolumeKey, DefaultInitialVolume), 0, 100);
        InitialMuted = Preferences.Get(InitialMutedKey, DefaultInitialMuted);
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

        // Sendspin buffer capacity
        Preferences.Set(SendspinBufferCapacityKey, SendspinBufferCapacity);

        // Save Sendspin audio formats as JSON
        var sendspinFormatsJson = System.Text.Json.JsonSerializer.Serialize(SendspinAudioFormats);
        Preferences.Set(SendspinAudioFormatsKey, sendspinFormatsJson);

        // Initial Volume
        Preferences.Set(InitialVolumeKey, InitialVolume);
        Preferences.Set(InitialMutedKey, InitialMuted);
    }

    public ClientCapabilities GetSendspinClientCapabilities()
    {
        var clientName = GetSendspinClientName();

        return new ClientCapabilities
        {
            ClientName = $"Mashin ({clientName})",
            ClientId = GetSendspinClientId(),
            ProductName = "Mashin Client",
            SoftwareVersion = "0.0.1",
            BufferCapacity = SendspinBufferCapacity,
            AudioFormats = SendspinAudioFormats,
            InitialVolume = InitialVolume,
            InitialMuted = InitialMuted,
        };
    }

    public string GetSendspinClientName()
    {
#if ANDROID || IOS
        return Microsoft.Maui.Devices.DeviceInfo.Name;
#else
        return Environment.MachineName;
#endif
    }

    public string GetSendspinClientId()
    {
        var compactClientName = GetSendspinClientName().Replace(" ", string.Empty).ToLowerInvariant();
        return $"mashin-{compactClientName}";
    }

    public string GetSendspinMusicAssistantPlayerId()
    {
        var sourceId = GetSendspinClientId();

        if (string.IsNullOrWhiteSpace(sourceId))
        {
            return string.Empty;
        }

        var normalized = sourceId.Trim();
        if (normalized.StartsWith("up", StringComparison.OrdinalIgnoreCase))
        {
            return normalized.ToLowerInvariant();
        }

        return string.Concat("up", normalized.Replace("-", string.Empty, StringComparison.Ordinal).ToLowerInvariant());
    }

    public int GetInitialVolume() => InitialVolume;

    public bool GetInitialMuted() => InitialMuted;

    public void SetInitialVolume(int volume)
    {
        var clamped = Math.Clamp(volume, 0, 100);
        if (InitialVolume == clamped)
        {
            return;
        }

        InitialVolume = clamped;
        Preferences.Set(InitialVolumeKey, InitialVolume);
    }

    public void SetInitialMuted(bool muted)
    {
        if (InitialMuted == muted)
        {
            return;
        }

        InitialMuted = muted;
        Preferences.Set(InitialMutedKey, InitialMuted);
    }

    public string GetSendspinPreferredAudioCodec()
    {
        var codec = SendspinAudioFormats.FirstOrDefault()?.Codec?.Trim().ToLowerInvariant();
        return codec switch
        {
            "opus" => "opus",
            "flac" => "flac",
            "pcm" => "pcm",
            _ => "opus",
        };
    }

    public bool SetSendspinPreferredAudioCodec(string codec)
    {
        var normalizedCodec = codec?.Trim().ToLowerInvariant();
        if (string.IsNullOrWhiteSpace(normalizedCodec))
        {
            return false;
        }

        var preferredFormats = BuildSendspinPreferredAudioFormats(normalizedCodec);
        if (preferredFormats == null)
        {
            return false;
        }

        var changed = SendspinAudioFormats.Count != preferredFormats.Count
            || SendspinAudioFormats.Count == 0
            || !string.Equals(SendspinAudioFormats[0].Codec, preferredFormats[0].Codec, StringComparison.OrdinalIgnoreCase);
        SendspinAudioFormats = preferredFormats;
        Save();
        return changed;
    }

    public ClientCapabilities GetClientCapabilities() => GetSendspinClientCapabilities();
    public string GetPreferredAudioCodec() => GetSendspinPreferredAudioCodec();
    public bool SetPreferredAudioCodec(string codec) => SetSendspinPreferredAudioCodec(codec);

    #endregion

    #region Helpers

    private static List<AudioFormat>? BuildSendspinPreferredAudioFormats(string codec)
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