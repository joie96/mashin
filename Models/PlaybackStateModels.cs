namespace mashin.Models;

public enum PlaybackStateKind
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
    public PlaybackStateKind State { get; set; } = PlaybackStateKind.Unknown;

    public DateTimeOffset ActiveSinceUtc { get; set; } = DateTimeOffset.UtcNow;

    public bool IsPending =>
        State == PlaybackStateKind.PendingToPlaying
        || State == PlaybackStateKind.PendingToPaused
        || State == PlaybackStateKind.PendingToNextTrack
        || State == PlaybackStateKind.PendingToPreviousTrack
        || State == PlaybackStateKind.PendingToSeek;
}
