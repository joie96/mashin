namespace mashin.Models;

using System;

/// <summary>
/// Playback control state used by the app-level player services.
/// </summary>
public enum PlayerStateType
{
    Unknown,
    Uninitialized,
    Error,
    Idle,
    Playing,
    Paused,
    Buffering,
    Seeking
}

/// <summary>
/// Unified player state with transition timestamp.
/// </summary>
public sealed class PlayerState
{
    public PlayerStateType State { get; set; } = PlayerStateType.Unknown;

    public DateTimeOffset ActiveSinceUtc { get; set; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Neutral audio format model for renderer-decoupled audio pipelines.
/// </summary>
public sealed class AudioFormatModel
{
    public required string Codec { get; init; }

    public required int SampleRate { get; init; }

    public required int Channels { get; init; }

    public int? BitDepth { get; init; }

    public int? Bitrate { get; init; }
}
