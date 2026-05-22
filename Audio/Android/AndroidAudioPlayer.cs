#if ANDROID
#pragma warning disable CA1416
#pragma warning disable CA1422
#pragma warning disable CS0618
using System;
using System.Threading;
using System.Threading.Tasks;
using Android.Media;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Models;

namespace mashin.Audio.Android
{
    public sealed class AndroidAudioPlayer : IAudioPlayer
    {
        private readonly ILogger<AndroidAudioPlayer> _logger;
        private Sendspin.SDK.Models.AudioFormat? _format;
        private IAudioSampleSource? _source;
        private AudioTrack? _audioTrack;
        private Thread? _playbackThread;
        private readonly object _playbackLock = new();
        private volatile bool _isPlaying;
        private volatile bool _disposed;

        private float _volume = 1.0f;
        private bool _isMuted;

        public AndroidAudioPlayer(ILogger<AndroidAudioPlayer> logger)
        {
            _logger = logger ?? throw new ArgumentNullException(nameof(logger));
            State = AudioPlayerState.Uninitialized;
            _logger.LogInformation("AndroidAudioPlayer instantiated");
        }

        public AudioPlayerState State { get; private set; }

        public float Volume
        {
            get => _volume;
            set
            {
                _volume = Math.Clamp(value, 0f, 1f);
            }
        }

        public bool IsMuted
        {
            get => _isMuted;
            set
            {
                _isMuted = value;
            }
        }

        public int OutputLatencyMs { get; private set; }

        public event EventHandler<AudioPlayerState>? StateChanged;
        public event EventHandler<AudioPlayerError>? ErrorOccurred;

        public Task InitializeAsync(Sendspin.SDK.Models.AudioFormat format, CancellationToken cancellationToken = default)
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

                    var channelConfig = format.Channels == 1
                        ? ChannelOut.Mono
                        : ChannelOut.Stereo;

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
                    SetState(AudioPlayerState.Stopped);

                    _logger.LogInformation(
                        "Android audio initialized: {SampleRate}Hz {Channels}ch, buffer={BufferSize}, latency≈{LatencyMs}ms",
                        format.SampleRate, format.Channels, bufferSize, OutputLatencyMs);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to initialize Android audio");
                    SetState(AudioPlayerState.Error);
                    ErrorOccurred?.Invoke(this, new AudioPlayerError("Failed to initialize audio", ex));
                    throw;
                }
            }, cancellationToken);
        }

        public void SetSampleSource(IAudioSampleSource source)
        {
            ArgumentNullException.ThrowIfNull(source);
            _source = source;
            _logger.LogInformation("Sample source configured: {SourceType}", source.GetType().FullName);
        }

        public void Play()
        {
            if (_audioTrack == null || _format == null)
                throw new InvalidOperationException("Not initialized");

            lock (_playbackLock)
            {
                if (_isPlaying)
                {
                    return;
                }

                _isPlaying = true;
                _audioTrack.SetVolume(1.0f);
                _audioTrack.Play();
                SetState(AudioPlayerState.Playing);

                _playbackThread = new Thread(PlaybackLoop)
                {
                    IsBackground = true,
                    Name = "AndroidAudioPlayback"
                };
                _playbackThread.Start();
            }

            _logger.LogInformation("Playback started");
        }

        public void Pause()
        {
            if (!_isPlaying) return;

            _isPlaying = false;
            _audioTrack?.Pause();
            _audioTrack?.Flush();
            JoinPlaybackThreadIfNeeded();
            SetState(AudioPlayerState.Paused);
            _logger.LogInformation("Playback paused");
        }

        public void Stop()
        {
            if (!_isPlaying) return;

            _isPlaying = false;
            _audioTrack?.Pause();
            _audioTrack?.Flush();
            _audioTrack?.Stop();
            JoinPlaybackThreadIfNeeded();
            SetState(AudioPlayerState.Stopped);
            _logger.LogInformation("Playback stopped");
        }

        private void PlaybackLoop()
        {
            var floatBuffer = new float[4096];
            var writeBuffer = new float[floatBuffer.Length];
            var diagnosticsLeft = 12;
            var sourceMissingLogged = false;
            var consecutiveZeroReads = 0;
            const int maxConsecutiveZeroReadsBeforeStop = 40;

            _logger.LogInformation("Playback loop entered (isPlaying={IsPlaying}, disposed={Disposed})", _isPlaying, _disposed);

            while (_isPlaying && !_disposed)
            {
                try
                {
                    if (_source == null || _isMuted)
                    {
                        if (_source == null && !sourceMissingLogged)
                        {
                            _logger.LogWarning("Playback loop running without sample source");
                            sourceMissingLogged = true;
                        }
                        Array.Clear(floatBuffer, 0, floatBuffer.Length);
                    }
                    else
                    {
                        sourceMissingLogged = false;
                        var read = _source.Read(floatBuffer, 0, floatBuffer.Length);

                        if (read == 0)
                        {
                            consecutiveZeroReads++;

                            if (consecutiveZeroReads == 1 || consecutiveZeroReads % 10 == 0)
                            {
                                _logger.LogInformation(
                                    "Audio source returned 0 samples (consecutive={Count})",
                                    consecutiveZeroReads);
                            }

                            if (consecutiveZeroReads >= maxConsecutiveZeroReadsBeforeStop)
                            {
                                _logger.LogWarning(
                                    "Stopping playback after {Count} consecutive zero-sample reads",
                                    consecutiveZeroReads);
                                Stop();
                                break;
                            }

                            Thread.Yield();
                            continue;
                        }

                        consecutiveZeroReads = 0;

                        if (read < floatBuffer.Length)
                            Array.Clear(floatBuffer, read, floatBuffer.Length - read);
                    }

                    var gain = _isMuted ? 0f : _volume;
                    if (gain <= 0f)
                    {
                        Array.Clear(writeBuffer, 0, writeBuffer.Length);
                    }
                    else
                    {
                        for (int i = 0; i < floatBuffer.Length; i++)
                        {
                            var sample = gain < 0.999f ? floatBuffer[i] * gain : floatBuffer[i];
                            writeBuffer[i] = Math.Clamp(sample, -1f, 1f);
                        }
                    }

                    var track = _audioTrack;
                    if (track == null)
                    {
                        throw new InvalidOperationException("AudioTrack not available");
                    }

                    var totalWritten = 0;
                    var offset = 0;
                    var remaining = writeBuffer.Length;

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

                    if (diagnosticsLeft > 0)
                    {
                        var firstSample = writeBuffer.Length > 0 ? writeBuffer[0] : 0f;
                        var peak = 0f;
                        for (int i = 0; i < writeBuffer.Length; i++)
                        {
                            var abs = Math.Abs(writeBuffer[i]);
                            if (abs > peak)
                            {
                                peak = abs;
                            }
                        }
                        _logger.LogInformation(
                            "Android audio write: samples={Samples}, written={Written}, gain={Gain:F2}, muted={Muted}, first={FirstSample}, peak={Peak}, playState={PlayState}",
                            writeBuffer.Length,
                            totalWritten,
                            gain,
                            _isMuted,
                            firstSample,
                            peak,
                            track.PlayState);
                        diagnosticsLeft--;
                    }
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Error in playback loop");
                    SetState(AudioPlayerState.Error);
                    ErrorOccurred?.Invoke(this, new AudioPlayerError("Playback error", ex));
                    break;
                }
            }

            _logger.LogInformation("Playback loop exited (isPlaying={IsPlaying}, disposed={Disposed})", _isPlaying, _disposed);
        }

        public ValueTask DisposeAsync()
        {
            if (_disposed) return ValueTask.CompletedTask;

            _disposed = true;
            _isPlaying = false;

            _playbackThread?.Join(1000);

            _audioTrack?.Stop();
            _audioTrack?.Release();
            _audioTrack?.Dispose();
            _audioTrack = null;

            SetState(AudioPlayerState.Uninitialized);
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

        private void SetState(AudioPlayerState newState)
        {
            if (State != newState)
            {
                State = newState;
                StateChanged?.Invoke(this, newState);
            }
        }

        Task IAudioPlayer.SwitchDeviceAsync(string? deviceId, CancellationToken cancellationToken)
        {
            throw new NotImplementedException();
        }
    }
}
#pragma warning restore CA1416
#pragma warning restore CA1422
#pragma warning restore CS0618
#endif