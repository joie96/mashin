using mashin.Models;
using mashin.Audio.Renderers;

namespace mashin.Audio;

/// <summary>
/// Exposes the current renderer state and state change notifications.
/// </summary>
public interface IAudioPlayerStateFeed
{
    PlayerStateType CurrentState { get; }

    event EventHandler<PlayerStateType>? StateChanged;
}

/// <summary>
/// Tracks <see cref="IAudioRenderer"/> state and forwards state updates.
/// </summary>
public sealed class AudioPlayerStateFeed : IAudioPlayerStateFeed, IDisposable
{
    private readonly IAudioRenderer _audioRenderer;

    public AudioPlayerStateFeed(IAudioRenderer audioRenderer)
    {
        _audioRenderer = audioRenderer;
        CurrentState = audioRenderer.State;
        _audioRenderer.StateChanged += OnAudioPlayerStateChanged;
    }

    public PlayerStateType CurrentState { get; private set; }

    public event EventHandler<PlayerStateType>? StateChanged;

    public void Dispose()
    {
        _audioRenderer.StateChanged -= OnAudioPlayerStateChanged;
    }

    private void OnAudioPlayerStateChanged(object? sender, PlayerStateType state)
    {
        CurrentState = state;
        StateChanged?.Invoke(this, state);
    }
}
