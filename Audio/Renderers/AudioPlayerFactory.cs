using mashin.Audio.Renderers;
#if ANDROID
using mashin.Audio.Renderers.Android;
#endif
#if WINDOWS
using mashin.Audio.Renderers.Windows;
#endif
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;

namespace mashin.Audio.Renderers;

/// <summary>
/// Creates platform-specific audio renderers and wraps them as an <see cref="IAudioPlayer"/>.
/// </summary>
public static class AudioPlayerFactory
{
    public static IAudioPlayer Create(ILoggerFactory loggerFactory)
    {
        IAudioRenderer renderer = CreateRenderer(loggerFactory);
        return Create(renderer, loggerFactory);
    }

    public static IAudioPlayer Create(IAudioRenderer renderer, ILoggerFactory loggerFactory)
    {
        return new SendspinPlayerRendererAdapter(
            renderer,
            loggerFactory.CreateLogger<SendspinPlayerRendererAdapter>());
    }

    public static IAudioRenderer CreateRenderer(ILoggerFactory loggerFactory)
    {
#if ANDROID
        return new AndroidAudioPlayer(loggerFactory.CreateLogger<AndroidAudioPlayer>());
#elif WINDOWS
        return new WasapiAudioPlayer(loggerFactory.CreateLogger<WasapiAudioPlayer>());
#else
        throw new PlatformNotSupportedException("Audio playback not supported on this platform");
#endif
    }
}
