using mashin.Models;

namespace mashin.Audio;

public interface IRendererSampleSource
{
    AudioFormatModel Format { get; }

    int Read(float[] buffer, int offset, int count);
}
