namespace mashin.Models;

public enum PlaybackStateType
{
    Unknown,
    Idle,
    Playing,
    Paused,
    Buffering,
    Seeking
}

public sealed class PlaybackStateCustom
{
    public PlaybackStateType State { get; set; } = PlaybackStateType.Unknown;

    public DateTimeOffset ActiveSinceUtc { get; set; } = DateTimeOffset.UtcNow;

    public bool IsPending =>
        State == PlaybackStateType.Buffering
        || State == PlaybackStateType.Seeking;
}
