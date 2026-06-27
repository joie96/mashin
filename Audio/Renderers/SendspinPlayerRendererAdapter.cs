using mashin.Audio.Renderers;
using mashin.Models;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Models;

namespace mashin.Audio.Renderers;

/// <summary>
/// Adapts an <see cref="IAudioRenderer"/> to the Sendspin <see cref="IAudioPlayer"/> contract.
/// </summary>
public sealed class SendspinPlayerRendererAdapter : IAudioPlayer
{
    private readonly IAudioRenderer _renderer;
    private readonly ILogger<SendspinPlayerRendererAdapter> _logger;

    public SendspinPlayerRendererAdapter(
        IAudioRenderer renderer,
        ILogger<SendspinPlayerRendererAdapter> logger)
    {
        _renderer = renderer;
        _logger = logger;

        _renderer.StateChanged += OnRendererStateChanged;
        _renderer.ErrorOccurred += OnRendererError;

        State = MapState(_renderer.State);
    }

    public AudioPlayerState State { get; private set; } = AudioPlayerState.Uninitialized;

    public float Volume
    {
        get => _renderer.Volume;
        set => _renderer.Volume = value;
    }

    public bool IsMuted
    {
        get => _renderer.IsMuted;
        set => _renderer.IsMuted = value;
    }

    public int OutputLatencyMs => _renderer.OutputLatencyMs;

    public event EventHandler<AudioPlayerState>? StateChanged;

    public event EventHandler<AudioPlayerError>? ErrorOccurred;

    public Task InitializeAsync(Sendspin.SDK.Models.AudioFormat format, CancellationToken cancellationToken = default)
    {
        var rendererFormat = new AudioFormatModel
        {
            Codec = string.IsNullOrWhiteSpace(format.Codec) ? "unknown" : format.Codec,
            SampleRate = format.SampleRate,
            Channels = format.Channels,
            BitDepth = format.BitDepth,
            Bitrate = format.Bitrate
        };

        return _renderer.InitializeAsync(rendererFormat, cancellationToken);
    }

    public void SetSampleSource(IAudioSampleSource source)
    {
        ArgumentNullException.ThrowIfNull(source);
        _renderer.SetSampleSource(new RendererSampleSourceAdapter(source));
    }

    public void Play()
    {
        _renderer.Play();
    }

    public void Pause()
    {
        _renderer.Pause();
    }

    public void Stop()
    {
        _renderer.Stop();
    }

    public Task SwitchDeviceAsync(string? deviceId, CancellationToken cancellationToken = default)
    {
        return _renderer.SwitchDeviceAsync(deviceId, cancellationToken);
    }

    public async ValueTask DisposeAsync()
    {
        _renderer.StateChanged -= OnRendererStateChanged;
        _renderer.ErrorOccurred -= OnRendererError;
        await _renderer.DisposeAsync();
    }

    private void OnRendererStateChanged(object? sender, PlayerStateType state)
    {
        var mapped = MapState(state);
        if (mapped == State)
        {
            return;
        }

        State = mapped;
        StateChanged?.Invoke(this, mapped);
    }

    private void OnRendererError(object? sender, Exception ex)
    {
        _logger.LogError(ex, "Audio renderer error propagated to Sendspin adapter.");
        ErrorOccurred?.Invoke(this, new AudioPlayerError("Audio renderer error", ex));
    }

    private static AudioPlayerState MapState(PlayerStateType state)
    {
        return state switch
        {
            PlayerStateType.Uninitialized => AudioPlayerState.Uninitialized,
            PlayerStateType.Playing => AudioPlayerState.Playing,
            PlayerStateType.Paused => AudioPlayerState.Paused,
            PlayerStateType.Error => AudioPlayerState.Error,
            PlayerStateType.Buffering => AudioPlayerState.Stopped,
            PlayerStateType.Seeking => AudioPlayerState.Stopped,
            PlayerStateType.Idle => AudioPlayerState.Stopped,
            _ => AudioPlayerState.Uninitialized
        };
    }

    private sealed class RendererSampleSourceAdapter : IAudioRendererSampleSource
    {
        private readonly IAudioSampleSource _source;

        public RendererSampleSourceAdapter(IAudioSampleSource source)
        {
            _source = source;
            var codec = string.IsNullOrWhiteSpace(source.Format.Codec) ? "unknown" : source.Format.Codec;
            Format = new AudioFormatModel
            {
                Codec = codec,
                SampleRate = source.Format.SampleRate,
                Channels = source.Format.Channels,
                BitDepth = source.Format.BitDepth,
                Bitrate = source.Format.Bitrate
            };
        }

        public AudioFormatModel Format { get; }

        public int Read(float[] buffer, int offset, int count)
        {
            return _source.Read(buffer, offset, count);
        }
    }
}
