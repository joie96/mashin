#if WINDOWS

// <copyright file="WasapiAudioPlayer.cs" company="Sendspin Windows Client">
// Licensed under the MIT License. See LICENSE file in the project root.
// </copyright>

using Microsoft.Extensions.Logging;
using NAudio.CoreAudioApi;
using NAudio.Wave;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Models;

namespace mashin.Audio.Windows;

/// <summary>
/// Windows WASAPI audio player using NAudio.
/// Provides low-latency audio output via WASAPI shared mode.
/// </summary>
/// <remarks>
/// <para>
/// Uses WASAPI shared mode for broad device compatibility. While exclusive mode
/// offers lower latency, shared mode is more reliable across different audio
/// hardware configurations and allows other applications to use audio simultaneously.
/// </para>
/// <para>
/// The 50ms latency setting provides a good balance between responsiveness and
/// stability. Lower values may cause glitches on some hardware.
/// </para>
/// </remarks>
public sealed class WasapiAudioPlayer : IAudioPlayer
{
    private const int RequestedLatencyMs = 200;

    private readonly ILogger<WasapiAudioPlayer> _logger;
    private string? _deviceId;
    private WasapiOut? _wasapiOut;
    private AudioSampleProviderAdapter? _sampleProvider;
    private AudioFormat? _format;
    private float _volume = 1.0f;
    private bool _isMuted;
    private int _outputLatencyMs;
    private bool _isOutputInitialized;
    private string _currentDeviceDisplayName = "System Default";

    /// <summary>
    /// Gets the detected output latency in milliseconds.
    /// This is the buffer latency reported by the WASAPI audio device.
    /// </summary>
    public int OutputLatencyMs => _outputLatencyMs;

    /// <inheritdoc/>
    public AudioPlayerState State { get; private set; } = AudioPlayerState.Uninitialized;

    /// <inheritdoc/>
    /// <remarks>
    /// Volume is applied in software via the sample provider rather than through
    /// WASAPI endpoint volume. This avoids COM threading issues and provides
    /// consistent behavior across different audio hardware.
    /// </remarks>
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

    /// <inheritdoc/>
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

    /// <inheritdoc/>
    public event EventHandler<AudioPlayerState>? StateChanged;

    /// <inheritdoc/>
    public event EventHandler<AudioPlayerError>? ErrorOccurred;

    /// <summary>
    /// Initializes a new instance of the <see cref="WasapiAudioPlayer"/> class.
    /// </summary>
    /// <param name="logger">Logger for diagnostics.</param>
    /// <param name="deviceId">
    /// Optional device ID for a specific audio output device.
    /// If null or empty, the system default device is used.
    /// </param>
    public WasapiAudioPlayer(ILogger<WasapiAudioPlayer> logger, string? deviceId = null)
    {
        _logger = logger;
        _deviceId = deviceId;
    }

    /// <inheritdoc/>
    public Task InitializeAsync(AudioFormat format, CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () =>
            {
                try
                {
                    _format = format;
                    CreateOutputForCurrentDevice();

                    SetState(AudioPlayerState.Stopped);
                    _logger.LogInformation(
                        "WASAPI player initialized: {SampleRate}Hz {Channels}ch, latency: {Latency}ms, device: {Device}",
                        format.SampleRate,
                        format.Channels,
                        _outputLatencyMs,
                        _currentDeviceDisplayName);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to initialize WASAPI player");
                    SetState(AudioPlayerState.Error);
                    ErrorOccurred?.Invoke(this, new AudioPlayerError("Failed to initialize audio output", ex));
                    throw;
                }
            },
            cancellationToken);
    }

    /// <inheritdoc/>
    public void SetSampleSource(IAudioSampleSource source)
    {
        if (_wasapiOut == null || _format == null)
        {
            throw new InvalidOperationException("Player not initialized. Call InitializeAsync first.");
        }

        ArgumentNullException.ThrowIfNull(source);

        // Create NAudio adapter with current volume/mute state
        _sampleProvider = new AudioSampleProviderAdapter(source, _format);
        _sampleProvider.Volume = _volume;
        _sampleProvider.IsMuted = _isMuted;

        // AudioClient can only be initialized once per WasapiOut instance.
        // Recreate the output device if we need to attach a new sample source.
        if (_isOutputInitialized)
        {
            _logger.LogDebug("WASAPI output already initialized. Recreating output before setting new source.");
            CreateOutputForCurrentDevice();
        }

        // Initialize WASAPI with our provider
        _wasapiOut.Init(_sampleProvider);
        _isOutputInitialized = true;

        _logger.LogDebug("Sample source configured");
    }

    /// <inheritdoc/>
    public void Play()
    {
        if (_wasapiOut == null || _sampleProvider == null)
        {
            throw new InvalidOperationException("Player not initialized or no sample source set.");
        }

        _wasapiOut.Play();
        SetState(AudioPlayerState.Playing);
        _logger.LogInformation("Playback started");
    }

    /// <inheritdoc/>
    public void Pause()
    {
        _wasapiOut?.Pause();
        SetState(AudioPlayerState.Paused);
        _logger.LogInformation("Playback paused");
    }

    /// <inheritdoc/>
    public void Stop()
    {
        _wasapiOut?.Stop();
        SetState(AudioPlayerState.Stopped);
        _logger.LogInformation("Playback stopped");
    }

    /// <inheritdoc/>
    public Task SwitchDeviceAsync(string? deviceId, CancellationToken cancellationToken = default)
    {
        return Task.Run(
            () =>
            {
                try
                {
                    // Remember current state
                    var wasPlaying = State == AudioPlayerState.Playing;
                    var currentSampleProvider = _sampleProvider;

                    _logger.LogInformation(
                        "Switching audio device from {OldDevice} to {NewDevice}",
                        _deviceId ?? "System Default",
                        deviceId ?? "System Default");

                    // Update device ID
                    _deviceId = deviceId;
                    CreateOutputForCurrentDevice();

                    // Re-attach sample provider if we had one
                    if (currentSampleProvider != null)
                    {
                        var output = _wasapiOut ?? throw new InvalidOperationException("Audio output is not available after device switch.");
                        output.Init(currentSampleProvider);
                        _isOutputInitialized = true;
                        _logger.LogDebug("Sample source re-attached to new device");
                    }

                    SetState(AudioPlayerState.Stopped);

                    // Resume playback if we were playing
                    if (wasPlaying && currentSampleProvider != null)
                    {
                        var output = _wasapiOut ?? throw new InvalidOperationException("Audio output is not available for playback after device switch.");
                        output.Play();
                        SetState(AudioPlayerState.Playing);
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
                    SetState(AudioPlayerState.Error);
                    ErrorOccurred?.Invoke(this, new AudioPlayerError("Failed to switch audio device", ex));
                    throw;
                }
            },
            cancellationToken);
    }

    /// <inheritdoc/>
    public async ValueTask DisposeAsync()
    {
        if (_wasapiOut != null)
        {
            _wasapiOut.PlaybackStopped -= OnPlaybackStopped;
            _wasapiOut.Stop();
            _wasapiOut.Dispose();
            _wasapiOut = null;
        }

        _isOutputInitialized = false;
        _sampleProvider = null;
        SetState(AudioPlayerState.Uninitialized);

        await Task.CompletedTask;
    }

    private void OnPlaybackStopped(object? sender, StoppedEventArgs e)
    {
        if (e.Exception != null)
        {
            _logger.LogError(e.Exception, "Playback stopped due to error");
            SetState(AudioPlayerState.Error);
            ErrorOccurred?.Invoke(this, new AudioPlayerError("Playback error", e.Exception));
        }
        else if (State == AudioPlayerState.Playing)
        {
            // Unexpected stop while playing
            SetState(AudioPlayerState.Stopped);
        }
    }

    private void SetState(AudioPlayerState newState)
    {
        if (State != newState)
        {
            _logger.LogDebug("Player state: {OldState} -> {NewState}", State, newState);
            State = newState;
            StateChanged?.Invoke(this, newState);
        }
    }

    private void CreateOutputForCurrentDevice()
    {
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