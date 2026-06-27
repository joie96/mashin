using mashin.Models;

namespace mashin.Audio.Renderers;

public interface IAudioRenderer : IAsyncDisposable
{
    PlayerStateType State { get; }

    float Volume { get; set; }

    bool IsMuted { get; set; }

    int OutputLatencyMs { get; }

    event EventHandler<PlayerStateType>? StateChanged;

    event EventHandler<Exception>? ErrorOccurred;

    Task InitializeAsync(AudioFormatModel format, CancellationToken cancellationToken = default);

    void SetSampleSource(IAudioRendererSampleSource source);

    void Play();

    void Pause();

    void Stop();

    Task SwitchDeviceAsync(string? deviceId, CancellationToken cancellationToken = default);
}

public interface IAudioRendererSampleSource
{
    AudioFormatModel Format { get; }

    int Read(float[] buffer, int offset, int count);
}