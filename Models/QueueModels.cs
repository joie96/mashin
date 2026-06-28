using mashin.Collections;

namespace mashin.Models;

public sealed class PlaybackQueue
{
    public string? QueueId { get; set; }

    public int? CurrentIndex { get; set; }
    public string? CurrentQueueItemId { get; set; }
    public int ItemCount { get; set; }

    public bool? ShuffleEnabled { get; set; }
    public RepeatMode? RepeatMode { get; set; }
    public bool? DontStopTheMusicEnabled { get; set; }

    public ObservableRangeCollection<QueueItem> Items { get; } = new();
}
