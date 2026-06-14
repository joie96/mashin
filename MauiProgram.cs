using FFImageLoading.Maui;
using CommunityToolkit.Maui;
using mashin.Audio;
using mashin.Services;
using mashin.ViewModels;
using DesktopAlbumDetailPage = mashin.Views.Desktop.AlbumDetailPage;
using DesktopArtistDetailPage = mashin.Views.Desktop.ArtistDetailPage;
using DesktopExplorePage = mashin.Views.Desktop.ExplorePage;
using DesktopFavoritesPage = mashin.Views.Desktop.FavoritesPage;
using DesktopHomePage = mashin.Views.Desktop.HomePage;
using DesktopMainPage = mashin.Views.Desktop.MainPage;
using DesktopPlaylistsPage = mashin.Views.Desktop.PlaylistsPage;
using DesktopPlaylistDetailPage = mashin.Views.Desktop.PlaylistDetailPage;
using DesktopSearchPage = mashin.Views.Desktop.SearchPage;
using MobileAlbumDetailPage = mashin.Views.Mobile.AlbumDetailPage;
using MobileArtistDetailPage = mashin.Views.Mobile.ArtistDetailPage;
using MobileExplorePage = mashin.Views.Mobile.ExplorePage;
using MobileFavoritesPage = mashin.Views.Mobile.FavoritesPage;
using MobileHomePage = mashin.Views.Mobile.HomePage;
using MobileMainPage = mashin.Views.Mobile.MainPage;
using MobilePlaylistsPage = mashin.Views.Mobile.PlaylistsPage;
using MobilePlaylistDetailPage = mashin.Views.Mobile.PlaylistDetailPage;
using MobileSearchPage = mashin.Views.Mobile.SearchPage;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using Sendspin.SDK.Audio;
using Sendspin.SDK.Client;
using Sendspin.SDK.Connection;
using Sendspin.SDK.Models;
using Sendspin.SDK.Synchronization;

namespace mashin;

public static class MauiProgram
{
    public static MauiApp CreateMauiApp()
    {
        var builder = MauiApp.CreateBuilder();
        builder
            .UseMauiApp<App>()
            .UseMauiCommunityToolkit()
            .UseFFImageLoading()
            .UseFluentMauiIcons()
            .UseFluentFilledMauiIcons()
            .ConfigureFonts(fonts =>
            {
                fonts.AddFont("OpenSans-Regular.ttf", "OpenSansRegular");
                fonts.AddFont("OpenSans-Semibold.ttf", "OpenSansSemibold");
                fonts.AddFont("Poppins-Regular.ttf", "PoppinsRegular");
                fonts.AddFont("Poppins-SemiBold.ttf", "PoppinsSemibold");
            });
#if DEBUG
        builder.Logging.AddDebug();

        builder.Logging.SetMinimumLevel(LogLevel.Warning);

        builder.Logging.AddFilter("Sendspin.SDK", LogLevel.Warning);
        builder.Logging.AddFilter("mashin", LogLevel.Warning);
        builder.Logging.AddFilter("mashin.Services.PlaybackService", LogLevel.Debug);
#else
        // Production Logging
        builder.Logging.SetMinimumLevel(LogLevel.Information);
#endif

        // Services registrieren
        builder.Services.AddSingleton<SettingsService>();
        builder.Services.AddSingleton<MusicAssistantService>();
        builder.Services.AddSingleton<IMusicAssistantEventHub, MusicAssistantEventHub>();
        builder.Services.AddSingleton<IUserDataService, UserDataService>();
        builder.Services.AddSingleton<IMediaItemActions, MediaItemActions>();
        builder.Services.AddSingleton<INavigationService, NavigationService>();
        builder.Services.AddSingleton<IOverlayService, OverlayService>();
        builder.Services.AddSingleton<IPlaybackService, PlaybackService>();

#if WINDOWS
        builder.Services.AddSingleton<IContextMenuService, WindowsContextMenuService>();
        builder.Services.AddSingleton<IKeyboardService, WindowsKeyboardService>();

#else
        builder.Services.AddSingleton<IContextMenuService, DefaultContextMenuService>();
        builder.Services.AddSingleton<IKeyboardService, DefaultKeyboardService>();

#endif


        // Audio services
        builder.Services.AddSingleton<ConnectionOptions>(_ => new ConnectionOptions
        {
            AutoReconnect = true
        });
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

        // Sendspin services
        builder.Services.AddSingleton<ClientCapabilities>(sp =>
        {
            var settings = sp.GetRequiredService<SettingsService>();
            return settings.GetSendspinClientCapabilities();
        });

        // Player services
        builder.Services.AddSingleton<ISendspinConnection, SendspinConnection>();
        builder.Services.AddSingleton<ISendspinClient, SendspinClientService>();
        builder.Services.AddSingleton<IPlayerService, SendspinPlayerService>();
        builder.Services.AddSingleton<IPlayerService, LocalDummyPlayerService>();
        builder.Services.AddSingleton<IPlayerService, RemotePlayerService>();


        // ViewModels registrieren
        builder.Services.AddSingleton<MainViewModel>();
        builder.Services.AddTransient<FavoritesViewModel>();
        builder.Services.AddTransient<HomeViewModel>();
        builder.Services.AddTransient<PlaylistsViewModel>();
        builder.Services.AddTransient<PlaylistDetailViewModel>();
        builder.Services.AddTransient<ArtistDetailViewModel>();
        builder.Services.AddTransient<AlbumDetailViewModel>();
        builder.Services.AddTransient<SearchViewModel>();

        // Views registrieren
        builder.Services.AddSingleton<DesktopMainPage>();
        builder.Services.AddSingleton<MobileMainPage>();
        builder.Services.AddTransient<DesktopHomePage>();
        builder.Services.AddTransient<MobileHomePage>();
        builder.Services.AddTransient<DesktopExplorePage>();
        builder.Services.AddTransient<MobileExplorePage>();
        builder.Services.AddTransient<DesktopFavoritesPage>();
        builder.Services.AddTransient<MobileFavoritesPage>();
        builder.Services.AddTransient<DesktopPlaylistsPage>();
        builder.Services.AddTransient<MobilePlaylistsPage>();
        builder.Services.AddTransient<DesktopPlaylistDetailPage>();
        builder.Services.AddTransient<MobilePlaylistDetailPage>();
        builder.Services.AddTransient<DesktopArtistDetailPage>();
        builder.Services.AddTransient<MobileArtistDetailPage>();
        builder.Services.AddTransient<DesktopAlbumDetailPage>();
        builder.Services.AddTransient<MobileAlbumDetailPage>();
        builder.Services.AddTransient<DesktopSearchPage>();
        builder.Services.AddTransient<MobileSearchPage>();

        var app = builder.Build();

        var startupLogger = app.Services
            .GetRequiredService<ILoggerFactory>()
            .CreateLogger("mashin.Startup");
        var resolvedPlayer = app.Services.GetRequiredService<IAudioPlayer>();
        startupLogger.LogInformation(
            "Resolved IAudioPlayer implementation: {AudioPlayerType}",
            resolvedPlayer.GetType().FullName);

        return app;
    }
}