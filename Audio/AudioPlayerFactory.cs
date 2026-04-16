using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;

namespace mashin.Audio
{
    public static class AudioPlayerFactory
    {
        public static IAudioPlayer Create(ILoggerFactory loggerFactory)
        {
#if ANDROID
            return new Audio.Android.AndroidAudioPlayer(loggerFactory.CreateLogger<Audio.Android.AndroidAudioPlayer>());
#elif WINDOWS
            return new Audio.Windows.WasapiAudioPlayer(loggerFactory.CreateLogger<Audio.Windows.WasapiAudioPlayer>());
#else
            throw new PlatformNotSupportedException("Audio playback not supported on this platform");
#endif
        }
    }
}