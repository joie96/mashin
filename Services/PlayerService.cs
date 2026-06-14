using mashin.Models;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Protocol.Messages;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

#region Interface

public interface IPlayerService : INotifyPropertyChanged, IAsyncDisposable
{
    PlaybackOutputMode OutputMode { get; }
    PlaybackStateModel PlayerState { get; }
    int Volume { get; }
    bool IsMuted { get; }

    Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default);
    Task DeactivateAsync();
    Task TogglePlayPauseAsync(CancellationToken cancellationToken = default);
    Task NextAsync(CancellationToken cancellationToken = default);
    Task PreviousAsync(CancellationToken cancellationToken = default);
    Task SeekAsync(double seconds, CancellationToken cancellationToken = default);
    Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default);
    Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default);
    Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default);
    Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default);
}

#endregion

#region Sendspin Player

public sealed class SendspinPlayerService : IPlayerService
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;
    private readonly ILogger<SendspinPlayerService> _logger;
    private readonly SettingsService _settingsService;
    private readonly ISendspinClient _sendspinClient;

    private bool _isConnected;
    private string? _connectedServerName;
    private string? _playerId;
    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private bool _isMuted;
    private string? _activePlayerId;

    #endregion

    #region Construction

    public SendspinPlayerService(
        MusicAssistantService musicAssistant,
        ILogger<SendspinPlayerService> logger,
        SettingsService settingsService,
        ISendspinClient sendspinClient)
    {
        _musicAssistant = musicAssistant;
        _logger = logger;
        _settingsService = settingsService;
        _sendspinClient = sendspinClient;
        _playerId = _settingsService.GetSendspinClientId();
        _activePlayerId = _playerId;

        _sendspinClient.PlayerStateChanged += OnSendspinPlayerStateChanged;
        _sendspinClient.ConnectionStateChanged += OnSendspinConnectionStateChanged;
    }

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.LocalSendspin;

    public bool IsConnected
    {
        get => _isConnected;
        private set => SetProperty(ref _isConnected, value);
    }

    public string? ConnectedServerName
    {
        get => _connectedServerName;
        private set => SetProperty(ref _connectedServerName, value);
    }

    public string? PlayerId
    {
        get => _playerId;
        private set => SetProperty(ref _playerId, value);
    }

    public PlaybackStateModel PlayerState
    {
        get => _playerState;
        set => SetProperty(ref _playerState, value);
    }

    public int Volume
    {
        get => _volume;
        private set => SetProperty(ref _volume, Math.Clamp(value, 0, 100));
    }

    public bool IsMuted
    {
        get => _isMuted;
        private set => SetProperty(ref _isMuted, value);
    }

    #endregion

    #region Commands

    public Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null)
    {
        return _sendspinClient.SendCommandAsync(command, parameters);
    }

    public Task UpdatePreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(codec))
        {
            return Task.CompletedTask;
        }

        _settingsService.SetSendspinPreferredAudioCodec(codec);
        return Task.CompletedTask;
    }

    public async Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        if (!string.IsNullOrWhiteSpace(targetPlayerId))
        {
            PlayerId = targetPlayerId;
        }

        _activePlayerId = PlayerId;

        if (!IsConnected
            && Uri.TryCreate(_settingsService.SendspinUrl, UriKind.Absolute, out var configuredServerUri))
        {
            await ConnectAsync(configuredServerUri, cancellationToken);
        }
    }

    public async Task DeactivateAsync()
    {
        _activePlayerId = null;
        await DisconnectAsync();
    }

    public async Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        if (string.IsNullOrWhiteSpace(musicAssistantPlayerId))
        {
            _logger.LogWarning("TogglePlayPause ignored: no active MA queue for Sendspin player {PlayerId}", _activePlayerId ?? PlayerId);
            return;
        }

        await _musicAssistant.PlayPauseAsync(musicAssistantPlayerId);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        if (string.IsNullOrWhiteSpace(musicAssistantPlayerId))
        {
            _logger.LogWarning("Next ignored: no active MA queue for Sendspin player {PlayerId}", _activePlayerId ?? PlayerId);
            return;
        }

        await _musicAssistant.NextAsync(musicAssistantPlayerId);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        if (string.IsNullOrWhiteSpace(musicAssistantPlayerId))
        {
            _logger.LogWarning("Previous ignored: no active MA queue for Sendspin player {PlayerId}", _activePlayerId ?? PlayerId);
            return;
        }

        await _musicAssistant.PreviousAsync(musicAssistantPlayerId);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        if (string.IsNullOrWhiteSpace(musicAssistantPlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SeekAsync(musicAssistantPlayerId, clamped);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        return _sendspinClient.SetVolumeAsync(Volume);
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        return _sendspinClient.SetMuteAsync(IsMuted);
    }

    public async Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        if (string.IsNullOrWhiteSpace(musicAssistantPlayerId))
        {
            _logger.LogWarning("SetShuffle ignored: no active MA queue for Sendspin player {PlayerId}", _activePlayerId ?? PlayerId);
            return;
        }

        await _musicAssistant.SetShuffleAsync(musicAssistantPlayerId, enabled);
    }

    public async Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default)
    {
        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        if (string.IsNullOrWhiteSpace(musicAssistantPlayerId))
        {
            _logger.LogWarning("SetRepeatMode ignored: no active MA queue for Sendspin player {PlayerId}", _activePlayerId ?? PlayerId);
            return;
        }

        await _musicAssistant.SetRepeatAsync(musicAssistantPlayerId, repeatMode);
    }

    public ValueTask DisposeAsync()
    {
        _sendspinClient.PlayerStateChanged -= OnSendspinPlayerStateChanged;
        _sendspinClient.ConnectionStateChanged -= OnSendspinConnectionStateChanged;
        return ValueTask.CompletedTask;
    }

    #endregion

    #region Connection

    private async Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken)
    {
        await _sendspinClient.ConnectAsync(serverUri, cancellationToken);
        ConnectedServerName = _sendspinClient.ServerName ?? serverUri.Host;
        IsConnected = _sendspinClient.ConnectionState == ConnectionState.Connected;
        PlayerId ??= _settingsService.GetSendspinClientId();
        _logger.LogInformation("Sendspin client connected to {Server}", ConnectedServerName);
    }

    private async Task DisconnectAsync()
    {
        await _sendspinClient.DisconnectAsync("client_disconnect");
        IsConnected = false;
        ConnectedServerName = null;
        PlayerState = new PlaybackStateModel(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    }

    #endregion

    #region Helpers

    private bool SetProperty<T>(ref T storage, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(storage, value))
        {
            return false;
        }

        storage = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

    #endregion

    #region Helpers

    private void OnSendspinPlayerStateChanged(object? sender, Sendspin.SDK.Models.PlayerState state)
    {
        Volume = state.Volume;
        IsMuted = state.Muted;
    }

    private void OnSendspinConnectionStateChanged(object? sender, ConnectionStateChangedEventArgs e)
    {
        IsConnected = e.NewState == ConnectionState.Connected;
        if (!IsConnected)
        {
            ConnectedServerName = null;
        }
    }

    #endregion
}

#endregion

#region Local Dummy Player

public sealed class LocalDummyPlayerService : IPlayerService
{
    #region Fields

    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private bool _isMuted;

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.LocalOffline;

    public PlaybackStateModel PlayerState
    {
        get => _playerState;
        private set => SetProperty(ref _playerState, value);
    }

    public int Volume
    {
        get => _volume;
        private set => SetProperty(ref _volume, Math.Clamp(value, 0, 100));
    }

    public bool IsMuted
    {
        get => _isMuted;
        private set => SetProperty(ref _isMuted, value);
    }

    #endregion

    #region Commands

    public Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public Task DeactivateAsync() => Task.CompletedTask;

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        var next = PlayerState.State == PlayerPlaybackState.Playing
            ? PlayerPlaybackState.Paused
            : PlayerPlaybackState.Playing;
        PlayerState = new PlaybackStateModel(next, DateTimeOffset.UtcNow);
        return Task.CompletedTask;
    }

    public Task NextAsync(CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        return Task.CompletedTask;
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        return Task.CompletedTask;
    }

    public Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public ValueTask DisposeAsync() => ValueTask.CompletedTask;

    #endregion

    #region Helpers

    private bool SetProperty<T>(ref T storage, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(storage, value))
        {
            return false;
        }

        storage = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

    #endregion

}

#endregion

#region Remote Player

public sealed class RemotePlayerService : IPlayerService
{
    #region Fields

    private readonly MusicAssistantService _musicAssistant;

    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private bool _isMuted;
    private string? _activePlayerId;

    #endregion

    #region Construction

    public RemotePlayerService(MusicAssistantService musicAssistant)
    {
        _musicAssistant = musicAssistant;
    }

    #endregion

    #region Events

    public event PropertyChangedEventHandler? PropertyChanged;

    #endregion

    #region Properties

    public PlaybackOutputMode OutputMode => PlaybackOutputMode.RemoteOnly;

    public PlaybackStateModel PlayerState
    {
        get => _playerState;
        private set => SetProperty(ref _playerState, value);
    }

    public int Volume
    {
        get => _volume;
        private set => SetProperty(ref _volume, Math.Clamp(value, 0, 100));
    }

    public bool IsMuted
    {
        get => _isMuted;
        private set => SetProperty(ref _isMuted, value);
    }

    #endregion

    #region Commands

    public Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _activePlayerId = targetPlayerId;
        return Task.CompletedTask;
    }
    public Task DeactivateAsync() => Task.CompletedTask;

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        var next = PlayerState.State == PlayerPlaybackState.Playing
            ? PlayerPlaybackState.Paused
            : PlayerPlaybackState.Playing;
        PlayerState = new PlaybackStateModel(next, DateTimeOffset.UtcNow);
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.PlayerPlayPauseAsync(_activePlayerId);
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        await _musicAssistant.PlayerNextAsync(_activePlayerId);
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        await _musicAssistant.PlayerPreviousAsync(_activePlayerId);
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.PlayerSeekAsync(_activePlayerId, clamped);
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerVolumeAsync(_activePlayerId, Volume);
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return Task.CompletedTask;
        }

        return _musicAssistant.SetPlayerMuteAsync(_activePlayerId, IsMuted);
    }

    public async Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetShuffleAsync(queueId, enabled);
    }

    public async Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetRepeatAsync(queueId, repeatMode);
    }

    public ValueTask DisposeAsync() => ValueTask.CompletedTask;

    #endregion

    #region Helpers

    private bool SetProperty<T>(ref T storage, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(storage, value))
        {
            return false;
        }

        storage = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

    private async Task<string?> ResolveQueueIdAsync()
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return null;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_activePlayerId);
        return queue?.QueueId;
    }

    #endregion
}

#endregion
