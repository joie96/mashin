namespace mashin.Services;

public enum PlayerPlaybackState
{
    Unknown,
    Stopped,
    Paused,
    Buffering,
    Playing,
    Seeking,
}

public sealed record PlaybackStateModel(PlayerPlaybackState State, DateTimeOffset TimestampUtc);