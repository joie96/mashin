using mashin.Models;
using Microsoft.Extensions.Logging;

#if WINDOWS
using NAudio.Wave;
#endif

#if ANDROID
using Android.Media;
#endif

namespace mashin.Audio.Sources;

/// <summary>
/// Extracts local audio into PCM chunks for sendspin pipeline playback.
/// </summary>
public sealed class LocalAudioChunkSource
{
    private readonly ILogger<LocalAudioChunkSource> _logger;

    public LocalAudioChunkSource(ILogger<LocalAudioChunkSource> logger)
    {
        _logger = logger;
    }

    public LocalAudioChunkStream ReadChunks(string sourcePath, double startSeconds = 0, int targetChunkMilliseconds = 20)
    {
        if (string.IsNullOrWhiteSpace(sourcePath))
        {
            throw new ArgumentException("Source path is required.", nameof(sourcePath));
        }

        var clampedStartSeconds = Math.Max(0, startSeconds);
        var chunkMs = Math.Max(5, targetChunkMilliseconds);

#if WINDOWS
        return ReadWindowsChunks(sourcePath, clampedStartSeconds, chunkMs);
#elif ANDROID
        return ReadAndroidChunks(sourcePath, clampedStartSeconds, chunkMs);
#else
        throw new PlatformNotSupportedException("Local file chunk extraction is currently implemented for Windows and Android only.");
#endif
    }

#if WINDOWS
    private LocalAudioChunkStream ReadWindowsChunks(string sourcePath, double startSeconds, int targetChunkMilliseconds)
    {
        if (!File.Exists(sourcePath))
        {
            throw new FileNotFoundException("Local audio file was not found.", sourcePath);
        }

        using var reader = new AudioFileReader(sourcePath);

        var durationSeconds = reader.TotalTime.TotalSeconds;
        if (startSeconds > 0)
        {
            var clamped = Math.Min(startSeconds, durationSeconds);
            reader.CurrentTime = TimeSpan.FromSeconds(clamped);
        }

        var pcmBytes = new List<byte>(256 * 1024);
        var readBuffer = new float[16384];

        int read;
        while ((read = reader.Read(readBuffer, 0, readBuffer.Length)) > 0)
        {
            pcmBytes.AddRange(FloatSamplesToPcm16Bytes(readBuffer.AsSpan(0, read)));
        }

        var format = new AudioFormatModel
        {
            Codec = "pcm",
            SampleRate = reader.WaveFormat.SampleRate,
            Channels = reader.WaveFormat.Channels,
            BitDepth = 16,
            Bitrate = reader.WaveFormat.SampleRate * reader.WaveFormat.Channels * 16
        };

        var chunks = SplitPcmChunks(
            pcmBytes.ToArray(),
            format.SampleRate,
            format.Channels,
            bitDepth: 16,
            targetChunkMilliseconds);

        _logger.LogInformation(
            "Prepared local Windows chunks. Source={Source}, StartSeconds={StartSeconds:F2}, DurationSeconds={DurationSeconds:F2}, Chunks={Chunks}, SampleRate={SampleRate}, Channels={Channels}",
            sourcePath,
            startSeconds,
            durationSeconds,
            chunks.Count,
            format.SampleRate,
            format.Channels);

        return new LocalAudioChunkStream(sourcePath, chunks, format, durationSeconds, startSeconds);
    }

    private static byte[] FloatSamplesToPcm16Bytes(ReadOnlySpan<float> samples)
    {
        if (samples.IsEmpty)
        {
            return Array.Empty<byte>();
        }

        var bytes = new byte[samples.Length * 2];
        var write = 0;
        for (var i = 0; i < samples.Length; i++)
        {
            var clamped = Math.Clamp(samples[i], -1f, 1f);
            var pcm = (short)Math.Round(clamped * short.MaxValue);
            bytes[write++] = (byte)(pcm & 0xFF);
            bytes[write++] = (byte)((pcm >> 8) & 0xFF);
        }

        return bytes;
    }
#endif

#if ANDROID
    private LocalAudioChunkStream ReadAndroidChunks(string sourcePath, double startSeconds, int targetChunkMilliseconds)
    {
        using var extractor = new MediaExtractor();
        extractor.SetDataSource(sourcePath);

        var trackIndex = SelectAudioTrack(extractor);
        if (trackIndex < 0)
        {
            throw new InvalidOperationException("No decodable audio track found in source.");
        }

        extractor.SelectTrack(trackIndex);
        if (startSeconds > 0)
        {
            var seekUs = (long)(startSeconds * 1_000_000d);
            extractor.SeekTo(seekUs, MediaExtractorSeekTo.ClosestSync);
        }

        var inputFormat = extractor.GetTrackFormat(trackIndex);
        var mime = inputFormat.GetString(MediaFormat.KeyMime);
        if (string.IsNullOrWhiteSpace(mime))
        {
            throw new InvalidOperationException("Audio track MIME type is missing.");
        }

        using var decoder = MediaCodec.CreateDecoderByType(mime);
        decoder.Configure(inputFormat, null, null, 0);
        decoder.Start();

        var pcmBytes = new List<byte>(256 * 1024);
        var bufferInfo = new MediaCodec.BufferInfo();
        var sawInputEos = false;
        var sawOutputEos = false;

        var outputSampleRate = inputFormat.ContainsKey(MediaFormat.KeySampleRate)
            ? inputFormat.GetInteger(MediaFormat.KeySampleRate)
            : 48000;
        var outputChannels = inputFormat.ContainsKey(MediaFormat.KeyChannelCount)
            ? inputFormat.GetInteger(MediaFormat.KeyChannelCount)
            : 2;
        var outputBitDepth = 16;

        while (!sawOutputEos)
        {
            if (!sawInputEos)
            {
                var inputIndex = decoder.DequeueInputBuffer(10_000);
                if (inputIndex >= 0)
                {
                    var inputBuffer = decoder.GetInputBuffer(inputIndex);
                    if (inputBuffer == null)
                    {
                        decoder.QueueInputBuffer(inputIndex, 0, 0, 0, MediaCodecBufferFlags.EndOfStream);
                        sawInputEos = true;
                    }
                    else
                    {
                        var sampleSize = extractor.ReadSampleData(inputBuffer, 0);
                        if (sampleSize < 0)
                        {
                            decoder.QueueInputBuffer(inputIndex, 0, 0, 0, MediaCodecBufferFlags.EndOfStream);
                            sawInputEos = true;
                        }
                        else
                        {
                            var presentationTimeUs = extractor.SampleTime;
                            decoder.QueueInputBuffer(inputIndex, 0, sampleSize, presentationTimeUs, 0);
                            extractor.Advance();
                        }
                    }
                }
            }

            var outputIndex = decoder.DequeueOutputBuffer(bufferInfo, 10_000);
            if (outputIndex == (int)MediaCodecInfoState.OutputFormatChanged)
            {
                var outputFormat = decoder.OutputFormat;
                if (outputFormat != null)
                {
                    if (outputFormat.ContainsKey(MediaFormat.KeySampleRate))
                    {
                        outputSampleRate = outputFormat.GetInteger(MediaFormat.KeySampleRate);
                    }

                    if (outputFormat.ContainsKey(MediaFormat.KeyChannelCount))
                    {
                        outputChannels = outputFormat.GetInteger(MediaFormat.KeyChannelCount);
                    }

                    if (outputFormat.ContainsKey(MediaFormat.KeyPcmEncoding))
                    {
                        var pcmEncoding = outputFormat.GetInteger(MediaFormat.KeyPcmEncoding);
                        outputBitDepth = pcmEncoding == (int)Encoding.PcmFloat ? 32 : 16;
                    }
                }

                continue;
            }

            if (outputIndex >= 0)
            {
                var outputBuffer = decoder.GetOutputBuffer(outputIndex);
                if (outputBuffer != null && bufferInfo.Size > 0)
                {
                    outputBuffer.Position(bufferInfo.Offset);
                    outputBuffer.Limit(bufferInfo.Offset + bufferInfo.Size);

                    var chunk = new byte[bufferInfo.Size];
                    outputBuffer.Get(chunk);

                    if (outputBitDepth == 32)
                    {
                        pcmBytes.AddRange(ConvertPcmFloatToPcm16(chunk));
                    }
                    else
                    {
                        pcmBytes.AddRange(chunk);
                    }
                }

                decoder.ReleaseOutputBuffer(outputIndex, false);

                if ((bufferInfo.Flags & MediaCodecBufferFlags.EndOfStream) != 0)
                {
                    sawOutputEos = true;
                }
            }
        }

        decoder.Stop();

        var durationUs = inputFormat.ContainsKey(MediaFormat.KeyDuration)
            ? inputFormat.GetLong(MediaFormat.KeyDuration)
            : 0L;

        var durationSeconds = durationUs > 0
            ? durationUs / 1_000_000d
            : outputChannels > 0 && outputSampleRate > 0
                ? (pcmBytes.Count / 2d) / (outputSampleRate * outputChannels)
                : 0d;

        var format = new AudioFormatModel
        {
            Codec = "pcm",
            SampleRate = outputSampleRate,
            Channels = outputChannels,
            BitDepth = 16,
            Bitrate = outputSampleRate * outputChannels * 16
        };

        var chunks = SplitPcmChunks(
            pcmBytes.ToArray(),
            format.SampleRate,
            format.Channels,
            bitDepth: 16,
            targetChunkMilliseconds);

        _logger.LogInformation(
            "Prepared local Android chunks. Source={Source}, StartSeconds={StartSeconds:F2}, DurationSeconds={DurationSeconds:F2}, Chunks={Chunks}, SampleRate={SampleRate}, Channels={Channels}",
            sourcePath,
            startSeconds,
            durationSeconds,
            chunks.Count,
            format.SampleRate,
            format.Channels);

        return new LocalAudioChunkStream(sourcePath, chunks, format, durationSeconds, startSeconds);
    }

    private static int SelectAudioTrack(MediaExtractor extractor)
    {
        for (var i = 0; i < extractor.TrackCount; i++)
        {
            var format = extractor.GetTrackFormat(i);
            var mime = format.GetString(MediaFormat.KeyMime);
            if (!string.IsNullOrWhiteSpace(mime) && mime.StartsWith("audio/", StringComparison.OrdinalIgnoreCase))
            {
                return i;
            }
        }

        return -1;
    }

    private static byte[] ConvertPcmFloatToPcm16(byte[] chunk)
    {
        if (chunk.Length < 4)
        {
            return Array.Empty<byte>();
        }

        var sampleCount = chunk.Length / 4;
        var output = new byte[sampleCount * 2];
        var write = 0;

        for (var i = 0; i + 3 < chunk.Length; i += 4)
        {
            var sample = BitConverter.ToSingle(chunk, i);
            var clamped = Math.Clamp(sample, -1f, 1f);
            var pcm = (short)Math.Round(clamped * short.MaxValue);
            output[write++] = (byte)(pcm & 0xFF);
            output[write++] = (byte)((pcm >> 8) & 0xFF);
        }

        return output;
    }
#endif

    private static IReadOnlyList<byte[]> SplitPcmChunks(
        byte[] pcmBytes,
        int sampleRate,
        int channels,
        int bitDepth,
        int targetChunkMilliseconds)
    {
        if (pcmBytes.Length == 0)
        {
            return Array.Empty<byte[]>();
        }

        if (sampleRate <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(sampleRate));
        }

        if (channels <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(channels));
        }

        var bytesPerSample = bitDepth / 8;
        if (bytesPerSample <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(bitDepth));
        }

        var bytesPerFrame = bytesPerSample * channels;
        var framesPerChunk = Math.Max(1, (sampleRate * targetChunkMilliseconds) / 1000);
        var chunkSize = Math.Max(bytesPerFrame, framesPerChunk * bytesPerFrame);

        var chunks = new List<byte[]>(Math.Max(1, pcmBytes.Length / Math.Max(chunkSize, 1)));
        var offset = 0;

        while (offset < pcmBytes.Length)
        {
            var remaining = pcmBytes.Length - offset;
            var size = Math.Min(chunkSize, remaining);
            var alignedSize = size - (size % bytesPerFrame);

            if (alignedSize <= 0)
            {
                break;
            }

            var chunk = new byte[alignedSize];
            Buffer.BlockCopy(pcmBytes, offset, chunk, 0, alignedSize);
            chunks.Add(chunk);
            offset += alignedSize;
        }

        return chunks;
    }
}

/// <summary>
/// Represents chunked local PCM audio plus format and timing metadata.
/// </summary>
public sealed record LocalAudioChunkStream(
    string SourcePath,
    IReadOnlyList<byte[]> Chunks,
    AudioFormatModel Format,
    double DurationSeconds,
    double StartSeconds);
