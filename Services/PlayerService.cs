using mashin.Models;
using Microsoft.Extensions.Logging;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace mashin.Services;

public enum PlayerMode
{
    LocalDummy,
    Sendspin,
    Remote
}

public enum PlayerRuntimeEventKind
{
    PlayingItemChanged
}

public sealed class PlayerRuntimeEventArgs : EventArgs
{
    public PlayerRuntimeEventArgs(PlayerRuntimeEventKind kind, QueueItem? playingItem = null)
    {
        Kind = kind;
        PlayingItem = playingItem;
        Timestamp = DateTimeOffset.UtcNow;
    }

    public PlayerRuntimeEventKind Kind { get; }
    public QueueItem? PlayingItem { get; }
    public DateTimeOffset Timestamp { get; }
}

public interface IPlayerService : INotifyPropertyChanged, IAsyncDisposable
{
    event EventHandler<PlayerRuntimeEventArgs>? RuntimeEvent;

    string ServiceId { get; }
    PlayerMode Mode { get; }
    PlaybackStateModel PlayerState { get; }
    QueueItem? PlayingItem { get; }
    int Volume { get; }
    bool IsMuted { get; }
    double DurationSeconds { get; }
    double PositionSeconds { get; }

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
    Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default);
    Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default);
    Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default);
    Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default);
}

public interface ISendspinPlayerService : IPlayerService
{
    bool IsConnected { get; }
    string? ConnectedServerName { get; }
    string? PlayerId { get; }

    new PlaybackStateModel PlayerState { get; set; }
    new int Volume { get; }
    new double DurationSeconds { get; }
    new double PositionSeconds { get; set; }
    string? TrackTitle { get; }
    string? TrackArtist { get; }
    string? TrackAlbum { get; }
    string? TrackImageUri { get; }
    new bool IsMuted { get; }
    bool? ShuffleEnabled { get; }
    string? RepeatMode { get; }

    Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken = default);
    Task DisconnectAsync();
    Task<bool> EnsureConnectedAsync(string? playerId, CancellationToken cancellationToken = default);
    Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null);
    Task UpdatePreferredAudioCodecAsync(string codec, CancellationToken cancellationToken = default);
}

public sealed class SendspinPlayerService : ISendspinPlayerService
{
    private readonly MusicAssistantService _musicAssistant;
    private readonly ILogger<SendspinPlayerService> _logger;
    private readonly SettingsService _settingsService;

    private bool _isConnected;
    private string? _connectedServerName;
    private string? _playerId;
    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private double _durationSeconds;
    private double _positionSeconds;
    private string? _trackTitle;
    private string? _trackArtist;
    private string? _trackAlbum;
    private string? _trackImageUri;
    private bool _isMuted;
    private bool? _shuffleEnabled;
    private string? _repeatMode;
    private string? _activePlayerId;
    private QueueItem? _playingItem;

    public SendspinPlayerService(
        MusicAssistantService musicAssistant,
        ILogger<SendspinPlayerService> logger,
        SettingsService settingsService)
    {
        _musicAssistant = musicAssistant;
        _logger = logger;
        _settingsService = settingsService;
        _playerId = BuildFallbackPlayerId();
        _activePlayerId = _playerId;
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    public event EventHandler<PlayerRuntimeEventArgs>? RuntimeEvent;

    public string ServiceId => "sendspin";
    public PlayerMode Mode => PlayerMode.Sendspin;

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

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public string? TrackTitle
    {
        get => _trackTitle;
        private set => SetProperty(ref _trackTitle, value);
    }

    public string? TrackArtist
    {
        get => _trackArtist;
        private set => SetProperty(ref _trackArtist, value);
    }

    public string? TrackAlbum
    {
        get => _trackAlbum;
        private set => SetProperty(ref _trackAlbum, value);
    }

    public string? TrackImageUri
    {
        get => _trackImageUri;
        private set => SetProperty(ref _trackImageUri, value);
    }

    public bool IsMuted
    {
        get => _isMuted;
        private set => SetProperty(ref _isMuted, value);
    }

    public bool? ShuffleEnabled
    {
        get => _shuffleEnabled;
        private set => SetProperty(ref _shuffleEnabled, value);
    }

    public string? RepeatMode
    {
        get => _repeatMode;
        private set => SetProperty(ref _repeatMode, value);
    }

    public QueueItem? PlayingItem
    {
        get => _playingItem;
        private set => SetPlayingItem(value);
    }

    public Task ConnectAsync(Uri serverUri, CancellationToken cancellationToken = default)
    {
        if (serverUri is null)
        {
            throw new ArgumentNullException(nameof(serverUri));
        }

        ConnectedServerName = serverUri.Host;
        IsConnected = true;
        PlayerId ??= BuildFallbackPlayerId();
        _logger.LogInformation("Thin Sendspin client connected to {Server}", ConnectedServerName);
        return Task.CompletedTask;
    }

    public Task DisconnectAsync()
    {
        IsConnected = false;
        PlayerState = new PlaybackStateModel(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
        return Task.CompletedTask;
    }

    public Task<bool> EnsureConnectedAsync(string? playerId, CancellationToken cancellationToken = default)
    {
        if (!string.IsNullOrWhiteSpace(playerId))
        {
            PlayerId = playerId;
        }

        return Task.FromResult(IsConnected);
    }

    public Task SendCommandAsync(string command, Dictionary<string, object>? parameters = null)
    {
        _logger.LogDebug("Thin Sendspin command pass-through requested: {Command}", command);
        return Task.CompletedTask;
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

    public Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        if (!string.IsNullOrWhiteSpace(targetPlayerId))
        {
            PlayerId = targetPlayerId;
        }

        _activePlayerId = PlayerId;
        return RefreshPlayingItemFromMusicAssistantAsync();
    }

    public Task DeactivateAsync()
    {
        return Task.CompletedTask;
    }

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        var next = PlayerState.State == PlayerPlaybackState.Playing
            ? PlayerPlaybackState.Paused
            : PlayerPlaybackState.Playing;
        PlayerState = new PlaybackStateModel(next, DateTimeOffset.UtcNow);
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerPlayPauseAsync(playerId));
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        PositionSeconds = 0;
        await ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerNextAsync(playerId));
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        PositionSeconds = 0;
        await ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerPreviousAsync(playerId));
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        PositionSeconds = seconds;
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerSeekAsync(playerId, clamped));
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.SetPlayerVolumeAsync(playerId, Volume));
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.SetPlayerMuteAsync(playerId, IsMuted));
    }

    public async Task SetShuffleAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        ShuffleEnabled = enabled;

        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetShuffleAsync(queueId, enabled);
    }

    public async Task SetRepeatModeAsync(mashin.Models.RepeatMode repeatMode, CancellationToken cancellationToken = default)
    {
        RepeatMode = repeatMode.ToString();

        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetRepeatAsync(queueId, repeatMode);
    }

    public async Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetDontStopTheMusicAsync(queueId, enabled);
    }

    public async Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.PlayIndexAsync(queueId, index);
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public async Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.MoveQueueItemAsync(queueId, queueItemId, posShift);
    }

    public async Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.DeleteQueueItemAsync(queueId, queueItemId);
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public ValueTask DisposeAsync()
    {
        return ValueTask.CompletedTask;
    }

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

    private string BuildFallbackPlayerId()
    {
        return _settingsService.GetSendspinClientId();
    }

    private async Task<string?> ResolveQueueIdAsync()
    {
        var playerId = _activePlayerId ?? PlayerId;
        if (string.IsNullOrWhiteSpace(playerId))
        {
            return null;
        }

        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(musicAssistantPlayerId);
        return queue?.QueueId;
    }

    private async Task ForwardToMusicAssistantAsync(Func<string, Task> operation)
    {
        var playerId = _activePlayerId ?? PlayerId;
        if (string.IsNullOrWhiteSpace(playerId))
        {
            return;
        }

        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        await operation(musicAssistantPlayerId);
    }

    private async Task RefreshPlayingItemFromMusicAssistantAsync()
    {
        var playerId = _activePlayerId ?? PlayerId;
        if (string.IsNullOrWhiteSpace(playerId))
        {
            PlayingItem = null;
            return;
        }

        var musicAssistantPlayerId = _settingsService.GetSendspinMusicAssistantPlayerId();
        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(musicAssistantPlayerId);
        PlayingItem = queue?.CurrentItem;
    }

    private bool SetPlayingItem(QueueItem? value)
    {
        var currentId = _playingItem?.QueueItemId;
        var nextId = value?.QueueItemId;
        if (string.Equals(currentId, nextId, StringComparison.Ordinal))
        {
            return false;
        }

        _playingItem = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(PlayingItem)));
        RaiseRuntimeEvent(PlayerRuntimeEventKind.PlayingItemChanged, value);
        return true;
    }

    private void RaiseRuntimeEvent(PlayerRuntimeEventKind kind, QueueItem? playingItem = null)
    {
        RuntimeEvent?.Invoke(this, new PlayerRuntimeEventArgs(kind, playingItem));
    }
}

public sealed class LocalDummyPlayerService : IPlayerService
{
    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private bool _isMuted;
    private double _durationSeconds;
    private double _positionSeconds;
    private QueueItem? _playingItem;

    public event PropertyChangedEventHandler? PropertyChanged;
    public event EventHandler<PlayerRuntimeEventArgs>? RuntimeEvent;

    public string ServiceId => "local";
    public PlayerMode Mode => PlayerMode.LocalDummy;

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

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        private set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public QueueItem? PlayingItem
    {
        get => _playingItem;
        private set => SetPlayingItem(value);
    }

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
        PositionSeconds = 0;
        return Task.CompletedTask;
    }

    public Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        PositionSeconds = 0;
        return Task.CompletedTask;
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        PositionSeconds = seconds;
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
    public Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default) => Task.CompletedTask;
    public Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        PlayingItem = new QueueItem { QueueItemId = $"local-{index}" };
        return Task.CompletedTask;
    }

    public Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        return Task.CompletedTask;
    }

    public ValueTask DisposeAsync() => ValueTask.CompletedTask;

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

    private bool SetPlayingItem(QueueItem? value)
    {
        var currentId = _playingItem?.QueueItemId;
        var nextId = value?.QueueItemId;
        if (string.Equals(currentId, nextId, StringComparison.Ordinal))
        {
            return false;
        }

        _playingItem = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(PlayingItem)));
        RaiseRuntimeEvent(PlayerRuntimeEventKind.PlayingItemChanged, value);
        return true;
    }

    private void RaiseRuntimeEvent(PlayerRuntimeEventKind kind, QueueItem? playingItem = null)
    {
        RuntimeEvent?.Invoke(this, new PlayerRuntimeEventArgs(kind, playingItem));
    }
}

public sealed class RemotePlayerService : IPlayerService
{
    private readonly MusicAssistantService _musicAssistant;

    private PlaybackStateModel _playerState = new(PlayerPlaybackState.Stopped, DateTimeOffset.UtcNow);
    private int _volume = 50;
    private bool _isMuted;
    private double _durationSeconds;
    private double _positionSeconds;
    private string? _activePlayerId;
    private QueueItem? _playingItem;

    public RemotePlayerService(MusicAssistantService musicAssistant)
    {
        _musicAssistant = musicAssistant;
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    public event EventHandler<PlayerRuntimeEventArgs>? RuntimeEvent;

    public string ServiceId => "remote";
    public PlayerMode Mode => PlayerMode.Remote;

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

    public double DurationSeconds
    {
        get => _durationSeconds;
        private set => SetProperty(ref _durationSeconds, Math.Max(0, value));
    }

    public double PositionSeconds
    {
        get => _positionSeconds;
        private set => SetProperty(ref _positionSeconds, Math.Max(0, value));
    }

    public QueueItem? PlayingItem
    {
        get => _playingItem;
        private set => SetPlayingItem(value);
    }

    public Task ActivateAsync(string? targetPlayerId, CancellationToken cancellationToken = default)
    {
        _activePlayerId = targetPlayerId;
        return RefreshPlayingItemFromMusicAssistantAsync();
    }
    public Task DeactivateAsync() => Task.CompletedTask;

    public Task TogglePlayPauseAsync(CancellationToken cancellationToken = default)
    {
        var next = PlayerState.State == PlayerPlaybackState.Playing
            ? PlayerPlaybackState.Paused
            : PlayerPlaybackState.Playing;
        PlayerState = new PlaybackStateModel(next, DateTimeOffset.UtcNow);
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerPlayPauseAsync(playerId));
    }

    public async Task NextAsync(CancellationToken cancellationToken = default)
    {
        PositionSeconds = 0;
        await ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerNextAsync(playerId));
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public async Task PreviousAsync(CancellationToken cancellationToken = default)
    {
        PositionSeconds = 0;
        await ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerPreviousAsync(playerId));
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public Task SeekAsync(double seconds, CancellationToken cancellationToken = default)
    {
        PositionSeconds = seconds;
        var clamped = Math.Max(0, (int)Math.Round(seconds));
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.PlayerSeekAsync(playerId, clamped));
    }

    public Task SetVolumeAsync(int volume, CancellationToken cancellationToken = default)
    {
        Volume = volume;
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.SetPlayerVolumeAsync(playerId, Volume));
    }

    public Task SetMutedAsync(bool muted, CancellationToken cancellationToken = default)
    {
        IsMuted = muted;
        return ForwardToMusicAssistantAsync(playerId => _musicAssistant.SetPlayerMuteAsync(playerId, IsMuted));
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

    public async Task SetDontStopTheMusicAsync(bool enabled, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.SetDontStopTheMusicAsync(queueId, enabled);
    }

    public async Task PlayQueueIndexAsync(int index, CancellationToken cancellationToken = default)
    {
        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.PlayIndexAsync(queueId, index);
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public async Task MoveQueueItemAsync(string queueItemId, int posShift, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.MoveQueueItemAsync(queueId, queueItemId, posShift);
    }

    public async Task DeleteQueueItemAsync(string queueItemId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(queueItemId))
        {
            return;
        }

        var queueId = await ResolveQueueIdAsync();
        if (string.IsNullOrWhiteSpace(queueId))
        {
            return;
        }

        await _musicAssistant.DeleteQueueItemAsync(queueId, queueItemId);
        await RefreshPlayingItemFromMusicAssistantAsync();
    }

    public ValueTask DisposeAsync() => ValueTask.CompletedTask;

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

    private async Task ForwardToMusicAssistantAsync(Func<string, Task> operation)
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            return;
        }

        await operation(_activePlayerId);
    }

    private async Task RefreshPlayingItemFromMusicAssistantAsync()
    {
        if (string.IsNullOrWhiteSpace(_activePlayerId))
        {
            PlayingItem = null;
            return;
        }

        var queue = await _musicAssistant.GetActiveQueueForPlayerAsync(_activePlayerId);
        PlayingItem = queue?.CurrentItem;
    }

    private bool SetPlayingItem(QueueItem? value)
    {
        var currentId = _playingItem?.QueueItemId;
        var nextId = value?.QueueItemId;
        if (string.Equals(currentId, nextId, StringComparison.Ordinal))
        {
            return false;
        }

        _playingItem = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(PlayingItem)));
        RaiseRuntimeEvent(PlayerRuntimeEventKind.PlayingItemChanged, value);
        return true;
    }

    private void RaiseRuntimeEvent(PlayerRuntimeEventKind kind, QueueItem? playingItem = null)
    {
        RuntimeEvent?.Invoke(this, new PlayerRuntimeEventArgs(kind, playingItem));
    }
}
