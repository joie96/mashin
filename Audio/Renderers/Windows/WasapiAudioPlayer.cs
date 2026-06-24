#if WINDOWS

using mashin.Audio;
using mashin.Audio;
using mashin.Models;
using Microsoft.Extensions.Logging;
using NAudio.CoreAudioApi;
using NAudio.Wave;

namespace mashin.Audio.Renderers.Windows;

/// <summary>
/// Windows WASAPI renderer using NAudio.
/// </summary>
public sealed class WasapiAudioPlayer : IAudioRenderer
{
    private const int RequestedLatencyMs = 200;

    private readonly ILogger<WasapiAudioPlayer> _logger;
    private string? _deviceId;
    private WasapiOut? _wasapiOut;
    private AudioSampleProviderAdapter? _sampleProvider;
    private AudioFormatModel? _format;
    private float _volume = 1.0f;
    private bool _isMuted;
    private int _outputLatencyMs;
    private bool _isOutputInitialized;
    private string _currentDeviceDisplayName = "System Default";
    private int _awaitingAudiblePlayback;

    public int OutputLatencyMs => _outputLatencyMs;

    public PlayerStateType State { get; private set; } = PlayerStateType.Uninitialized;

    public float Volume
    {
        get => _volume;
        set
        {
            _volume = Math.Clamp(value, 0f, 1f);
            if (_sampleProvider != null)
            {
                _sampleProvider.Volume = _volume;
            }
        }
    }

    public bool IsMuted
    {
        get => _isMuted;
        set
        {
            _isMuted = value;
            if (_sampleProvider != null)
            {
                _sampleProvider.IsMuted = value;
            }
        }
    }

    public event EventHandler<PlayerStateType>? StateChanged;

    public event EventHandler<Exception>? ErrorOccurred;

    public WasapiAudioPlayer(ILogger<WasapiAudioPlayer> logger, string? deviceId = null)
    {
        _logger = logger;
        _deviceId = deviceId;
    }

    public Task InitializeAsync(AudioFormatModel format, CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () =>
            {
                try
                {
                    _format = format;
                    CreateOutputForCurrentDevice();

                    SetState(PlayerStateType.Idle);
                    _logger.LogInformation(
                        "WASAPI renderer initialized: {SampleRate}Hz {Channels}ch, latency: {Latency}ms, device: {Device}",
                        format.SampleRate,
                        format.Channels,
                        _outputLatencyMs,
                        _currentDeviceDisplayName);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to initialize WASAPI renderer");
                    SetState(PlayerStateType.Error);
                    ErrorOccurred?.Invoke(this, ex);
                    throw;
                }
            },
            cancellationToken);
    }

    public void SetSampleSource(IRendererSampleSource source)
    {
        if (_wasapiOut == null || _format == null)
        {
            throw new InvalidOperationException("Renderer not initialized. Call InitializeAsync first.");
        }

        ArgumentNullException.ThrowIfNull(source);

        _sampleProvider = new AudioSampleProviderAdapter(source, _format)
        {
            Volume = _volume,
            IsMuted = _isMuted
        };
        _sampleProvider.AudibleSamplesRendered += OnAudibleSamplesRendered;

        if (_isOutputInitialized)
        {
            _logger.LogDebug("WASAPI output already initialized. Recreating output before setting new source.");
            CreateOutputForCurrentDevice();
        }

        _wasapiOut.Init(_sampleProvider);
        _isOutputInitialized = true;
        _logger.LogDebug("Sample source configured");
    }

    public void Play()
    {
        if (_wasapiOut == null || _sampleProvider == null)
        {
            throw new InvalidOperationException("Renderer not initialized or no sample source set.");
        }

        Interlocked.Exchange(ref _awaitingAudiblePlayback, 1);
        _sampleProvider.ResetAudibleSampleDetection();
        SetState(PlayerStateType.Buffering);
        _wasapiOut.Play();
        _logger.LogInformation("Playback started, waiting for first audible audio before setting Playing state");
    }

    public void Pause()
    {
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);
        _wasapiOut?.Pause();
        SetState(PlayerStateType.Paused);
        _logger.LogInformation("Playback paused");
    }

    public void Stop()
    {
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);
        _wasapiOut?.Stop();
        SetState(PlayerStateType.Idle);
        _logger.LogInformation("Playback stopped");
    }

    public Task SwitchDeviceAsync(string? deviceId, CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () =>
            {
                try
                {
                    var wasPlaying = State == PlayerStateType.Playing || State == PlayerStateType.Buffering;
                    var currentSampleProvider = _sampleProvider;

                    _logger.LogInformation(
                        "Switching audio device from {OldDevice} to {NewDevice}",
                        _deviceId ?? "System Default",
                        deviceId ?? "System Default");

                    _deviceId = deviceId;
                    CreateOutputForCurrentDevice();

                    if (currentSampleProvider != null)
                    {
                        var output = _wasapiOut ?? throw new InvalidOperationException("Audio output is not available after device switch.");
                        output.Init(currentSampleProvider);
                        _isOutputInitialized = true;
                        _logger.LogDebug("Sample source re-attached to new device");
                    }

                    SetState(PlayerStateType.Idle);

                    if (wasPlaying && currentSampleProvider != null)
                    {
                        Interlocked.Exchange(ref _awaitingAudiblePlayback, 1);
                        currentSampleProvider.ResetAudibleSampleDetection();
                        var output = _wasapiOut ?? throw new InvalidOperationException("Audio output is not available for playback after device switch.");
                        SetState(PlayerStateType.Buffering);
                        output.Play();
                        _logger.LogInformation("Playback resumed on new device");
                    }

                    _logger.LogInformation(
                        "Audio device switched successfully: {Device}, latency: {Latency}ms",
                        _currentDeviceDisplayName,
                        _outputLatencyMs);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to switch audio device");
                    SetState(PlayerStateType.Error);
                    ErrorOccurred?.Invoke(this, ex);
                    throw;
                }
            },
            cancellationToken);
    }

    public async ValueTask DisposeAsync()
    {
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);

        if (_wasapiOut != null)
        {
            _wasapiOut.PlaybackStopped -= OnPlaybackStopped;
            _wasapiOut.Stop();
            _wasapiOut.Dispose();
            _wasapiOut = null;
        }

        _isOutputInitialized = false;
        if (_sampleProvider != null)
        {
            _sampleProvider.AudibleSamplesRendered -= OnAudibleSamplesRendered;
        }

        _sampleProvider = null;
        SetState(PlayerStateType.Uninitialized);

        await Task.CompletedTask;
    }

    private void OnPlaybackStopped(object? sender, StoppedEventArgs e)
    {
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);

        if (e.Exception != null)
        {
            _logger.LogError(e.Exception, "Playback stopped due to error");
            SetState(PlayerStateType.Error);
            ErrorOccurred?.Invoke(this, e.Exception);
        }
        else if (State == PlayerStateType.Playing || State == PlayerStateType.Buffering)
        {
            SetState(PlayerStateType.Idle);
        }
    }

    private void SetState(PlayerStateType newState)
    {
        if (State != newState)
        {
            _logger.LogDebug("Renderer state: {OldState} -> {NewState}", State, newState);
            State = newState;
            StateChanged?.Invoke(this, newState);
        }
    }

    private void OnAudibleSamplesRendered()
    {
        if (Interlocked.CompareExchange(ref _awaitingAudiblePlayback, 0, 1) != 1)
        {
            return;
        }

        SetState(PlayerStateType.Playing);
        _logger.LogDebug("First audible audio rendered; renderer state set to Playing");
    }

    private void CreateOutputForCurrentDevice()
    {
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);

        if (_wasapiOut != null)
        {
            _wasapiOut.PlaybackStopped -= OnPlaybackStopped;
            _wasapiOut.Stop();
            _wasapiOut.Dispose();
            _wasapiOut = null;
        }

        var device = ResolveDevice();
        if (device != null)
        {
            _wasapiOut = new WasapiOut(device, AudioClientShareMode.Shared, useEventSync: true, latency: RequestedLatencyMs);
            _currentDeviceDisplayName = device.FriendlyName;
        }
        else
        {
            _wasapiOut = new WasapiOut(AudioClientShareMode.Shared, useEventSync: true, latency: RequestedLatencyMs);
            _currentDeviceDisplayName = "System Default";
        }

        _wasapiOut.PlaybackStopped += OnPlaybackStopped;
        _outputLatencyMs = RequestedLatencyMs;
        _isOutputInitialized = false;
    }

    private MMDevice? ResolveDevice()
    {
        if (string.IsNullOrEmpty(_deviceId))
        {
            return null;
        }

        try
        {
            using var enumerator = new MMDeviceEnumerator();
            var device = enumerator.GetDevice(_deviceId);
            _logger.LogInformation("Using audio device: {DeviceName}", device.FriendlyName);
            return device;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to get device {DeviceId}, falling back to default", _deviceId);
            return null;
        }
    }
}
#endif
