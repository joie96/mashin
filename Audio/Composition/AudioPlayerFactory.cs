using mashin.Audio;
using mashin.Audio.Adapters;
using mashin.Audio.Renderers.Android;
using mashin.Audio.Renderers.Windows;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;

namespace mashin.Audio.Composition
{
    public static class AudioPlayerFactory
    {
        public static IAudioPlayer Create(ILoggerFactory loggerFactory)
        {
            IAudioRenderer renderer = CreateRenderer(loggerFactory);
            return new SendspinAudioPlayerAdapter(
                renderer,
                loggerFactory.CreateLogger<SendspinAudioPlayerAdapter>());
        }

        private static IAudioRenderer CreateRenderer(ILoggerFactory loggerFactory)
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
}
