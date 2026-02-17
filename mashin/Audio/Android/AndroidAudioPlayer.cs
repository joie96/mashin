#if ANDROID
using System;
using System.Threading;
using System.Threading.Tasks;
using Android.Media;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;

namespace mashin.Audio.Android
{
    public sealed class AndroidAudioPlayer : IAudioPlayer
    {
        private readonly ILogger<AndroidAudioPlayer> _logger;
        private Sendspin.SDK.Models.AudioFormat? _format;
        private IAudioSampleSource? _source;
        private AudioTrack? _audioTrack;
        private Thread? _playbackThread;
        private bool _isPlaying;
        private bool _disposed;

        private float _volume = 1.0f;
        private bool _isMuted;

        public AndroidAudioPlayer(ILogger<AndroidAudioPlayer> logger)
        {
            _logger = logger ?? throw new ArgumentNullException(nameof(logger));
            State = AudioPlayerState.Uninitialized;
        }

        public AudioPlayerState State { get; private set; }

        public float Volume
        {
            get => _volume;
            set
            {
                _volume = Math.Clamp(value, 0f, 1f);
                _audioTrack?.SetVolume(_volume);
            }
        }

        public bool IsMuted
        {
            get => _isMuted;
            set
            {
                _isMuted = value;
                _audioTrack?.SetVolume(_isMuted ? 0f : _volume);
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

                    _audioTrack = new AudioTrack.Builder()!
                        .SetAudioAttributes(new AudioAttributes.Builder()!
                            .SetUsage(AudioUsageKind.Media)!
                            .SetContentType(AudioContentType.Music)!
                            .Build()!)!
                        .SetAudioFormat(new AudioFormat.Builder()!
                            .SetEncoding(Encoding.PcmFloat)!
                            .SetSampleRate(format.SampleRate)!
                            .SetChannelMask(channelConfig)!
                            .Build()!)!
                        .SetBufferSizeInBytes(bufferSize * 4)!
                        .Build();

                    OutputLatencyMs = (bufferSize * 1000) / format.SampleRate;
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
            _logger.LogDebug("Sample source configured");
        }

        public void Play()
        {
            if (_audioTrack == null || _format == null)
                throw new InvalidOperationException("Not initialized");

            if (_isPlaying) return;

            _isPlaying = true;
            _audioTrack.Play();
            SetState(AudioPlayerState.Playing);

            _playbackThread = new Thread(PlaybackLoop) { IsBackground = true };
            _playbackThread.Start();

            _logger.LogInformation("Playback started");
        }

        public void Pause()
        {
            if (!_isPlaying) return;

            _isPlaying = false;
            _audioTrack?.Pause();
            SetState(AudioPlayerState.Paused);
            _logger.LogInformation("Playback paused");
        }

        public void Stop()
        {
            if (!_isPlaying) return;

            _isPlaying = false;
            _audioTrack?.Stop();
            SetState(AudioPlayerState.Stopped);
            _logger.LogInformation("Playback stopped");
        }

        private void PlaybackLoop()
        {
            var buffer = new float[4096];

            while (_isPlaying && !_disposed)
            {
                try
                {
                    if (_source == null || _isMuted)
                    {
                        Array.Clear(buffer, 0, buffer.Length);
                    }
                    else
                    {
                        var read = _source.Read(buffer, 0, buffer.Length);
                        if (read == 0)
                        {
                            Stop();
                            break;
                        }

                        if (read < buffer.Length)
                            Array.Clear(buffer, read, buffer.Length - read);

                        // Apply volume
                        if (_volume < 0.999f)
                        {
                            for (int i = 0; i < read; i++)
                                buffer[i] *= _volume;
                        }
                    }

                    _audioTrack?.Write(buffer, 0, buffer.Length, WriteMode.Blocking);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Error in playback loop");
                    SetState(AudioPlayerState.Error);
                    ErrorOccurred?.Invoke(this, new AudioPlayerError("Playback error", ex));
                    break;
                }
            }
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
#endif