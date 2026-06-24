#if ANDROID
#pragma warning disable CA1416
#pragma warning disable CA1422
#pragma warning disable CS0618

using Android.Media;
using Android.OS;
using mashin.Audio;
using mashin.Audio;
using mashin.Models;
using Microsoft.Extensions.Logging;

namespace mashin.Audio.Renderers.Android;

public sealed class AndroidAudioPlayer : IAudioRenderer
{
    private const float AudibleSampleThreshold = 0.0001f;

    private readonly ILogger<AndroidAudioPlayer> _logger;
    private AudioFormatModel? _format;
    private IRendererSampleSource? _source;
    private AudioTrack? _audioTrack;
    private Thread? _playbackThread;
    private readonly object _playbackLock = new();
    private volatile bool _isPlaying;
    private volatile bool _disposed;
    private int _awaitingAudiblePlayback;

    private float _volume = 1.0f;
    private bool _isMuted;

    public AndroidAudioPlayer(ILogger<AndroidAudioPlayer> logger)
    {
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        State = PlayerStateType.Uninitialized;
        _logger.LogInformation("AndroidAudioPlayer instantiated");
    }

    public PlayerStateType State { get; private set; }

    public float Volume
    {
        get => _volume;
        set => _volume = Math.Clamp(value, 0f, 1f);
    }

    public bool IsMuted
    {
        get => _isMuted;
        set => _isMuted = value;
    }

    public int OutputLatencyMs { get; private set; }

    public event EventHandler<PlayerStateType>? StateChanged;

    public event EventHandler<Exception>? ErrorOccurred;

    public Task InitializeAsync(AudioFormatModel format, CancellationToken cancellationToken = default)
    {
        return Task.Run(() =>
        {
            try
            {
                ArgumentNullException.ThrowIfNull(format);

                if (_disposed)
                {
                    _logger.LogInformation("Re-initializing AndroidAudioPlayer after previous dispose");
                    _disposed = false;
                }

                _isPlaying = false;
                _playbackThread?.Join(300);
                _playbackThread = null;

                _audioTrack?.Release();
                _audioTrack?.Dispose();
                _audioTrack = null;

                _format = format;

                var channelConfig = format.Channels == 1 ? ChannelOut.Mono : ChannelOut.Stereo;

                var bufferSize = AudioTrack.GetMinBufferSize(
                    format.SampleRate,
                    channelConfig,
                    Encoding.PcmFloat);

                if (bufferSize <= 0)
                {
                    throw new InvalidOperationException($"Invalid buffer size: {bufferSize}");
                }

                _audioTrack = new AudioTrack(
                    global::Android.Media.Stream.Music,
                    format.SampleRate,
                    channelConfig,
                    Encoding.PcmFloat,
                    bufferSize * 4,
                    AudioTrackMode.Stream);

                var bytesPerSample = 4;
                OutputLatencyMs = (bufferSize * 1000) / (format.SampleRate * format.Channels * bytesPerSample);
                SetState(PlayerStateType.Idle);

                _logger.LogInformation(
                    "Android audio initialized: {SampleRate}Hz {Channels}ch, buffer={BufferSize}, latency≈{LatencyMs}ms",
                    format.SampleRate,
                    format.Channels,
                    bufferSize,
                    OutputLatencyMs);
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Failed to initialize Android audio");
                SetState(PlayerStateType.Error);
                ErrorOccurred?.Invoke(this, ex);
                throw;
            }
        }, cancellationToken);
    }

    public void SetSampleSource(IRendererSampleSource source)
    {
        ArgumentNullException.ThrowIfNull(source);
        _source = source;
        _logger.LogInformation("Sample source configured: {SourceType}", source.GetType().FullName);
    }

    public void Play()
    {
        if (_audioTrack == null || _format == null)
        {
            throw new InvalidOperationException("Not initialized");
        }

        lock (_playbackLock)
        {
            if (_isPlaying)
            {
                return;
            }

            _isPlaying = true;
            Interlocked.Exchange(ref _awaitingAudiblePlayback, 1);
            _audioTrack.SetVolume(1.0f);
            _audioTrack.Play();

            _playbackThread = new Thread(PlaybackLoop)
            {
                IsBackground = true,
                Name = "AndroidAudioPlayback"
            };
            _playbackThread.Start();
        }

        SetState(PlayerStateType.Buffering);
        _logger.LogInformation("Playback started, waiting for first audible audio before setting Playing state");
    }

    public void Pause()
    {
        if (!_isPlaying)
        {
            return;
        }

        _isPlaying = false;
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);
        _audioTrack?.Pause();
        _audioTrack?.Flush();
        JoinPlaybackThreadIfNeeded();
        SetState(PlayerStateType.Paused);
        _logger.LogInformation("Playback paused");
    }

    public void Stop()
    {
        if (!_isPlaying)
        {
            return;
        }

        _isPlaying = false;
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);
        _audioTrack?.Pause();
        _audioTrack?.Flush();
        _audioTrack?.Stop();
        JoinPlaybackThreadIfNeeded();
        SetState(PlayerStateType.Idle);
        _logger.LogInformation("Playback stopped");
    }

    private void PlaybackLoop()
    {
        try
        {
            Process.SetThreadPriority(global::Android.OS.ThreadPriority.Audio);
            _logger.LogDebug("Set Android playback thread priority to Audio");
        }
        catch (Exception ex)
        {
            _logger.LogDebug(ex, "Failed to set Android playback thread priority");
        }

        var floatBuffer = new float[4096];
        var writeBuffer = new float[floatBuffer.Length];

        while (_isPlaying && !_disposed)
        {
            try
            {
                if (_source == null || _isMuted)
                {
                    Array.Clear(floatBuffer, 0, floatBuffer.Length);
                }
                else
                {
                    var read = _source.Read(floatBuffer, 0, floatBuffer.Length);
                    if (read == 0)
                    {
                        Thread.Yield();
                        continue;
                    }

                    if (read < floatBuffer.Length)
                    {
                        Array.Clear(floatBuffer, read, floatBuffer.Length - read);
                    }
                }

                var gain = _isMuted ? 0f : _volume;
                if (gain <= 0f)
                {
                    Array.Clear(writeBuffer, 0, writeBuffer.Length);
                }
                else
                {
                    for (var i = 0; i < floatBuffer.Length; i++)
                    {
                        var sample = gain < 0.999f ? floatBuffer[i] * gain : floatBuffer[i];
                        writeBuffer[i] = Math.Clamp(sample, -1f, 1f);
                    }
                }

                var hasAudibleSamples = false;
                if (gain > 0f)
                {
                    for (var i = 0; i < writeBuffer.Length; i++)
                    {
                        if (Math.Abs(writeBuffer[i]) >= AudibleSampleThreshold)
                        {
                            hasAudibleSamples = true;
                            break;
                        }
                    }
                }

                var track = _audioTrack ?? throw new InvalidOperationException("AudioTrack not available");
                var remaining = writeBuffer.Length;
                var offset = 0;
                var totalWritten = 0;

                while (remaining > 0 && _isPlaying && !_disposed)
                {
                    var written = OperatingSystem.IsAndroidVersionAtLeast(23)
                        ? track.Write(writeBuffer, offset, remaining, WriteMode.Blocking)
                        : track.Write(writeBuffer, offset, remaining, WriteMode.Blocking);

                    if (written < 0)
                    {
                        throw new InvalidOperationException($"AudioTrack write failed: {written}");
                    }

                    if (written == 0)
                    {
                        Thread.Sleep(1);
                        continue;
                    }

                    offset += written;
                    remaining -= written;
                    totalWritten += written;
                }

                if (hasAudibleSamples
                    && totalWritten > 0
                    && Interlocked.CompareExchange(ref _awaitingAudiblePlayback, 0, 1) == 1)
                {
                    SetState(PlayerStateType.Playing);
                }
            }
            catch (Exception ex)
            {
                Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);
                _logger.LogError(ex, "Error in playback loop");
                SetState(PlayerStateType.Error);
                ErrorOccurred?.Invoke(this, ex);
                break;
            }
        }
    }

    public Task SwitchDeviceAsync(string? deviceId, CancellationToken cancellationToken = default)
    {
        throw new NotImplementedException();
    }

    public ValueTask DisposeAsync()
    {
        if (_disposed)
        {
            return ValueTask.CompletedTask;
        }

        _disposed = true;
        _isPlaying = false;
        Interlocked.Exchange(ref _awaitingAudiblePlayback, 0);

        _playbackThread?.Join(1000);

        _audioTrack?.Stop();
        _audioTrack?.Release();
        _audioTrack?.Dispose();
        _audioTrack = null;

        SetState(PlayerStateType.Uninitialized);
        return ValueTask.CompletedTask;
    }

    private void JoinPlaybackThreadIfNeeded()
    {
        Thread? threadToJoin;
        lock (_playbackLock)
        {
            threadToJoin = _playbackThread;
            _playbackThread = null;
        }

        if (threadToJoin == null || threadToJoin == Thread.CurrentThread)
        {
            return;
        }

        if (!threadToJoin.Join(300))
        {
            _logger.LogWarning("Playback thread did not stop within timeout");
        }
    }

    private void SetState(PlayerStateType newState)
    {
        if (State == newState)
        {
            return;
        }

        State = newState;
        StateChanged?.Invoke(this, newState);
    }
}

#pragma warning restore CA1416
#pragma warning restore CA1422
#pragma warning restore CS0618
#endif
