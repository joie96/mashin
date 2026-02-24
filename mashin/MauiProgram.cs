using FFImageLoading.Maui;
using mashin.Audio;
using mashin.Services;
using mashin.ViewModels;
using mashin.Views.Desktop;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Synchronization;

namespace mashin;

public static class MauiProgram
{
    public static MauiApp CreateMauiApp()
    {
        var builder = MauiApp.CreateBuilder();
        builder
            .UseMauiApp<App>()
            .UseFFImageLoading()
            .UseFluentMauiIcons()
            .UseFluentFilledMauiIcons()
            .ConfigureFonts(fonts =>
            {
                fonts.AddFont("OpenSans-Regular.ttf", "OpenSansRegular");
                fonts.AddFont("OpenSans-Semibold.ttf", "OpenSansSemibold");
            });
#if DEBUG
        builder.Logging.AddDebug();
        builder.Logging.SetMinimumLevel(LogLevel.Debug);

        builder.Logging.AddFilter("Sendspin.SDK.Client.SendspinClientService", LogLevel.Warning);
        builder.Logging.AddFilter("Sendspin.SDK", LogLevel.Warning);
        builder.Logging.AddFilter("mashin", LogLevel.Debug);
#else
        // Production Logging
        builder.Logging.SetMinimumLevel(LogLevel.Information);
#endif

        // Services registrieren
        builder.Services.AddSingleton<SettingsService>();
        builder.Services.AddSingleton<MusicAssistantService>();
        builder.Services.AddSingleton<IUserDataService, UserDataService>();
        builder.Services.AddSingleton<IMediaItemActions, MediaItemActions>();
        builder.Services.AddSingleton<INavigationService, NavigationService>();
        builder.Services.AddSingleton<IOverlayService, OverlayService>();
        builder.Services.AddSingleton<IPlaylistStoreService, PlaylistStoreService>();
        builder.Services.AddSingleton<IQueueSyncService, QueueSyncService>();

#if WINDOWS
        builder.Services.AddSingleton<IContextMenuService, WindowsContextMenuService>();
        builder.Services.AddSingleton<IKeyboardService, WindowsKeyboardService>();

#else
        builder.Services.AddSingleton<IContextMenuService, DefaultContextMenuService>();
        builder.Services.AddSingleton<IKeyboardService, DefaultKeyboardService>();

#endif


        // Sendspin components
        builder.Services.AddSingleton<IClockSynchronizer, KalmanClockSynchronizer>();
        builder.Services.AddSingleton<IAudioPlayer>(sp =>
        {
            var loggerFactory = sp.GetRequiredService<ILoggerFactory>();
            return AudioPlayerFactory.Create(loggerFactory);
        });
        builder.Services.AddSingleton<IAudioDecoderFactory, AudioDecoderFactory>();
        builder.Services.AddSingleton<IAudioPipeline>(sp =>
        {
            var logger = sp.GetRequiredService<ILogger<AudioPipeline>>();
            var decoderFactory = sp.GetRequiredService<IAudioDecoderFactory>();
            var clockSync = sp.GetRequiredService<IClockSynchronizer>();

            return new AudioPipeline(
                logger,
                decoderFactory,
                clockSync,
                bufferFactory: (format, sync) =>
                {
                    var buffer = new UntimedAudioBuffer(format, bufferCapacityMs: 30000);
                    buffer.TargetBufferMilliseconds = 500; 
                    return buffer;
                },
                playerFactory: () => sp.GetRequiredService<IAudioPlayer>(),
                sourceFactory: (buffer, timeFunc) =>
                {
                    return new UntimedAudioSampleSource((UntimedAudioBuffer)buffer);
                });
        });
        builder.Services.AddSingleton<IPlayerService, PlayerService>();


        // ViewModels registrieren
        builder.Services.AddSingleton<MainViewModel>();
        builder.Services.AddTransient<FavoritesViewModel>();
        builder.Services.AddTransient<PlaylistDetailViewModel>();
        builder.Services.AddTransient<ArtistDetailViewModel>();
        builder.Services.AddTransient<AlbumDetailViewModel>();
        builder.Services.AddTransient<SearchViewModel>();

        // Views registrieren
        builder.Services.AddSingleton<MainPage>();
        builder.Services.AddTransient<HomePage>();
        builder.Services.AddTransient<ExplorePage>();
        builder.Services.AddTransient<FavoritesPage>();
        builder.Services.AddTransient<PlaylistDetailPage>();
        builder.Services.AddTransient<ArtistDetailPage>();
        builder.Services.AddTransient<AlbumDetailPage>();
        builder.Services.AddTransient<SearchPage>();

        return builder.Build();
    }
}