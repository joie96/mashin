using Sendspin.SDK.Audio;
using Sendspin.SDK.Models;
using System.Threading;

namespace mashin.Audio;

/// <summary>
/// Lock-free FIFO audio buffer without time synchronization.
/// Uses atomic operations for thread-safe single-producer single-consumer access.
/// Optimized to prevent audio thread blocking during UI operations.
/// </summary>
public sealed class UntimedAudioBuffer : ITimedAudioBuffer
{
    private readonly float[] _buffer;
    private readonly int _bufferCapacitySamples;
    
    private volatile int _writePos;
    private volatile int _readPos;
    private int _currentSamples;
    
    private long _totalWritten;
    private long _totalRead;
    private long _underrunCount;
    private bool _disposed;

    /// <inheritdoc/>
    public AudioFormat Format { get; }

    /// <inheritdoc/>
    public SyncCorrectionOptions SyncOptions => SyncCorrectionOptions.Default;

    /// <inheritdoc/>
    public double TargetBufferMilliseconds { get; set; }

    /// <inheritdoc/>
    public double BufferedMilliseconds
    {
        get
        {
            var samples = Interlocked.CompareExchange(ref _currentSamples, 0, 0);
            var samplesPerMs = (Format.SampleRate * Format.Channels) / 1000.0;
            return samples / samplesPerMs;
        }
    }

    /// <inheritdoc/>
    public bool IsReadyForPlayback => BufferedMilliseconds >= TargetBufferMilliseconds * 0.8;

    /// <inheritdoc/>
    public long OutputLatencyMicroseconds { get; set; }

    /// <inheritdoc/>
    public long CalibratedStartupLatencyMicroseconds { get; set; }

    /// <inheritdoc/>
    public string? TimingSourceName { get; set; }

    /// <inheritdoc/>
    public long SyncErrorMicroseconds => 0;

    /// <inheritdoc/>
    public double SmoothedSyncErrorMicroseconds => 0;

    /// <inheritdoc/>
    [Obsolete]
    public double TargetPlaybackRate => 1.0;

        /// <inheritdoc/>
        [Obsolete]
    #pragma warning disable CS0067
        public event Action<double>? TargetPlaybackRateChanged;
    #pragma warning restore CS0067

    /// <summary>
    /// Initializes a new instance of the <see cref="UntimedAudioBuffer"/> class.
    /// </summary>
    /// <param name="format">Audio format specification.</param>
    /// <param name="bufferCapacityMs">Buffer capacity in milliseconds.</param>
    public UntimedAudioBuffer(AudioFormat format, int bufferCapacityMs)
    {
        Format = format ?? throw new ArgumentNullException(nameof(format));
        TargetBufferMilliseconds = 250;
        
        var samplesPerMs = (format.SampleRate * format.Channels) / 1000.0;
        _bufferCapacitySamples = (int)(samplesPerMs * bufferCapacityMs);
        _buffer = new float[_bufferCapacitySamples];
    }

    /// <inheritdoc/>
    public void Write(ReadOnlySpan<float> samples, long serverTimestamp)
    {
        if (samples.IsEmpty) return;
        
        // Batch write - minimize atomic operations
        var writePos = _writePos;
        var samplesToWrite = samples.Length;
        var written = 0;
        
        while (written < samplesToWrite)
        {
            var chunk = Math.Min(samplesToWrite - written, _bufferCapacitySamples - writePos);
            samples.Slice(written, chunk).CopyTo(_buffer.AsSpan(writePos, chunk));
            writePos = (writePos + chunk) % _bufferCapacitySamples;
            written += chunk;
        }
        
        _writePos = writePos;
        
        // Update counters atomically only once at the end
        var newSampleCount = Interlocked.Add(ref _currentSamples, samplesToWrite);
        Interlocked.Add(ref _totalWritten, samplesToWrite);
        
        // Handle overflow - drop oldest samples if buffer full
        if (newSampleCount > _bufferCapacitySamples)
        {
            var overflow = newSampleCount - _bufferCapacitySamples;
            _readPos = (_readPos + overflow) % _bufferCapacitySamples;
            Interlocked.Add(ref _currentSamples, -overflow);
        }
    }

    /// <inheritdoc/>
    [Obsolete]
    public int Read(Span<float> buffer, long currentLocalTime)
    {
        return ReadRaw(buffer, currentLocalTime);
    }

    /// <inheritdoc/>
    public int ReadRaw(Span<float> buffer, long currentLocalTime)
    {
        // Lock-free read for single consumer (audio thread)
        var currentSamples = Interlocked.CompareExchange(ref _currentSamples, 0, 0);
        var toRead = Math.Min(buffer.Length, currentSamples);
        var samplesRead = 0;
        
        while (samplesRead < toRead)
        {
            var readPos = _readPos;
            var chunk = Math.Min(toRead - samplesRead, _bufferCapacitySamples - readPos);
            _buffer.AsSpan(readPos, chunk).CopyTo(buffer.Slice(samplesRead));
            _readPos = (readPos + chunk) % _bufferCapacitySamples;
            samplesRead += chunk;
        }
        
        if (samplesRead > 0)
        {
            Interlocked.Add(ref _currentSamples, -samplesRead);
            Interlocked.Add(ref _totalRead, samplesRead);
        }
        else if (buffer.Length > 0)
        {
            Interlocked.Increment(ref _underrunCount);
        }

        if (samplesRead < buffer.Length)
        {
            buffer.Slice(samplesRead).Fill(0f);
        }

        return samplesRead;
    }

    /// <inheritdoc/>
    public void NotifyExternalCorrection(int samplesDropped, int samplesInserted)
    {
        // No correction needed
    }

    /// <inheritdoc/>
    public void Clear()
    {
        _writePos = 0;
        _readPos = 0;
        Interlocked.Exchange(ref _currentSamples, 0);
    }

    /// <inheritdoc/>
    public AudioBufferStats GetStats()
    {
        var currentSamples = Interlocked.CompareExchange(ref _currentSamples, 0, 0);
        var samplesPerMs = (Format.SampleRate * Format.Channels) / 1000.0;
        return new AudioBufferStats
        {
            BufferedMs = currentSamples / samplesPerMs,
            TargetMs = TargetBufferMilliseconds,
            UnderrunCount = Interlocked.Read(ref _underrunCount),
            OverrunCount = 0,
            DroppedSamples = 0,
            TotalSamplesWritten = Interlocked.Read(ref _totalWritten),
            TotalSamplesRead = Interlocked.Read(ref _totalRead),
            SyncErrorMicroseconds = 0,
            SmoothedSyncErrorMicroseconds = 0,
            IsPlaybackActive = Interlocked.Read(ref _totalRead) > 0,
            SamplesDroppedForSync = 0,
            SamplesInsertedForSync = 0,
            CurrentCorrectionMode = SyncCorrectionMode.None,
            TargetPlaybackRate = 1.0,
            SamplesReadSinceStart = Interlocked.Read(ref _totalRead),
            SamplesOutputSinceStart = Interlocked.Read(ref _totalRead),
            ElapsedSinceStartMs = 0,
        };
    }

    /// <inheritdoc/>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        Clear();
    }
}
