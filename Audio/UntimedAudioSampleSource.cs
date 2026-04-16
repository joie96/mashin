using Sendspin.SDK.Audio;
using Sendspin.SDK.Models;

namespace mashin.Audio;

/// <summary>
/// Simple audio sample source without time synchronization.
/// Reads directly from UntimedAudioBuffer without any timestamp handling.
/// </summary>
public sealed class UntimedAudioSampleSource : IAudioSampleSource
{
    private readonly UntimedAudioBuffer _buffer;

    /// <inheritdoc/>
    public AudioFormat Format => _buffer.Format;

    /// <summary>
    /// Initializes a new instance of the <see cref="UntimedAudioSampleSource"/> class.
    /// </summary>
    /// <param name="buffer">The untimed audio buffer to read from.</param>
    public UntimedAudioSampleSource(UntimedAudioBuffer buffer)
    {
        ArgumentNullException.ThrowIfNull(buffer);
        _buffer = buffer;
    }

    /// <inheritdoc/>
    public int Read(float[] buffer, int offset, int count)
    {
        // Read directly from buffer without any time calls
        var span = buffer.AsSpan(offset, count);
        var read = _buffer.ReadRaw(span, 0); // timestamp ignored by UntimedAudioBuffer

        // Fill remainder with silence if underrun
        if (read < count)
        {
            buffer.AsSpan(offset + read, count - read).Fill(0f);
        }

        // Always return requested count to keep NAudio happy
        return count;
    }
}
