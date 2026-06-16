namespace mashin.Models;

public enum PlaybackStateType
{
    Unknown,
    Idle,
    Playing,
    Paused,
    Buffering,
    PendingToPlaying,
    PendingToPaused,
    PendingToNextTrack,
    PendingToPreviousTrack,
    PendingToSeek
}

public sealed class PlaybackStateCustom
{
    public PlaybackStateType State { get; set; } = PlaybackStateType.Unknown;

    public DateTimeOffset ActiveSinceUtc { get; set; } = DateTimeOffset.UtcNow;

    public bool IsPending =>
        State == PlaybackStateType.PendingToPlaying
        || State == PlaybackStateType.PendingToPaused
        || State == PlaybackStateType.PendingToNextTrack
        || State == PlaybackStateType.PendingToPreviousTrack
        || State == PlaybackStateType.PendingToSeek;
}
