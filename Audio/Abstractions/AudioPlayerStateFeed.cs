using Sendspin.SDK.Audio;

namespace mashin.Audio.Abstractions;

public interface IAudioPlayerStateFeed
{
    AudioPlayerState CurrentState { get; }

    event EventHandler<AudioPlayerState>? StateChanged;
}

public sealed class AudioPlayerStateFeed : IAudioPlayerStateFeed, IDisposable
{
    private readonly IAudioPlayer _audioPlayer;

    public AudioPlayerStateFeed(IAudioPlayer audioPlayer)
    {
        _audioPlayer = audioPlayer;
        CurrentState = audioPlayer.State;
        _audioPlayer.StateChanged += OnAudioPlayerStateChanged;
    }

    public AudioPlayerState CurrentState { get; private set; }

    public event EventHandler<AudioPlayerState>? StateChanged;

    public void Dispose()
    {
        _audioPlayer.StateChanged -= OnAudioPlayerStateChanged;
    }

    private void OnAudioPlayerStateChanged(object? sender, AudioPlayerState state)
    {
        CurrentState = state;
        StateChanged?.Invoke(this, state);
    }
}
