using FFImageLoading.Maui;
using CommunityToolkit.Maui;
using mashin.Audio;
using mashin.Audio.Pipeline;
using mashin.Audio.Renderers;
using mashin.Audio.Sources;
using mashin.Logging;
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
using Microsoft.Extensions.Logging.Console;
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
        var logDirectory = ResolveLogDirectory();
        var logFilePath = Path.Combine(logDirectory, $"mashin-{DateTime.Now:yyyyMMdd}.log");

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
        builder.Logging.AddConsoleFormatter<CustomConsoleFormatter, SimpleConsoleFormatterOptions>(options =>
        {
            options.TimestampFormat = "HH:mm:ss.fff";
        });

        builder.Logging.AddConsole(options =>
        {
            options.FormatterName = CustomConsoleFormatter.FormatterName;
        });

        builder.Logging.AddProvider(new FileLoggerProvider(logFilePath));

        builder.Logging.SetMinimumLevel(LogLevel.Debug);
#else
        builder.Logging.AddConsoleFormatter<CustomConsoleFormatter, SimpleConsoleFormatterOptions>(options =>
        {
            options.TimestampFormat = "HH:mm:ss.fff";
        });

        builder.Logging.AddConsole(options =>
        {
            options.FormatterName = CustomConsoleFormatter.FormatterName;
        });

        builder.Logging.AddProvider(new FileLoggerProvider(logFilePath));

        // Production Logging
        builder.Logging.SetMinimumLevel(LogLevel.Debug);
#endif

        // Reduce noisy transport/debug traces from Sendspin in all configurations.
        builder.Logging.AddFilter("Sendspin", LogLevel.Information);
        builder.Logging.AddFilter("Sendspin.SDK", LogLevel.Information);
        builder.Logging.AddFilter("Sendspin.SDK.Connection", LogLevel.None);
        builder.Logging.AddFilter("Sendspin.SDK.Connection.SendspinConnection", LogLevel.None);

        // Services registrieren
        builder.Services.AddSingleton<SettingsService>();
        builder.Services.AddSingleton<MusicAssistantService>();
        builder.Services.AddSingleton<IPlaylistService, PlaylistService>();
        builder.Services.AddSingleton<IMusicAssistantEventHub, MusicAssistantEventHub>();
        builder.Services.AddSingleton<IConnectionService, ConnectionService>();
        builder.Services.AddSingleton<IUserDataService, UserDataService>();
        builder.Services.AddSingleton<IMediaItemActions, MediaItemActions>();
        builder.Services.AddSingleton<INavigationService, NavigationService>();
        builder.Services.AddSingleton<IOverlayService, OverlayService>();
        builder.Services.AddSingleton<PlaybackService>();

#if WINDOWS
        builder.Services.AddSingleton<IContextMenuService, WindowsContextMenuService>();
        builder.Services.AddSingleton<IKeyboardService, WindowsKeyboardService>();

#else
        builder.Services.AddSingleton<IContextMenuService, DefaultContextMenuService>();
        builder.Services.AddSingleton<IKeyboardService, DefaultKeyboardService>();

#endif


        // Audio renderer
        builder.Services.AddSingleton<IAudioRenderer>(sp =>
        {
            var loggerFactory = sp.GetRequiredService<ILoggerFactory>();
            return AudioPlayerFactory.CreateRenderer(loggerFactory);
        });

        // Sendspin audio player derived from renderer
        builder.Services.AddSingleton<IAudioPlayer>(sp =>
        {
            var loggerFactory = sp.GetRequiredService<ILoggerFactory>();
            var renderer = sp.GetRequiredService<IAudioRenderer>();
            return AudioPlayerFactory.Create(renderer, loggerFactory);
        });


        // Sendspin audio pipeline
        builder.Services.AddSingleton<IClockSynchronizer, KalmanClockSynchronizer>();
        builder.Services.AddSingleton<IAudioPlayerStateFeed, AudioPlayerStateFeed>();
        builder.Services.AddSingleton<IAudioDecoderFactory, AudioDecoderFactory>();
        builder.Services.AddKeyedSingleton<IAudioPipeline>("sendspin", (sp, _) =>
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

        // Local audio pipeline
        builder.Services.AddSingleton<LocalAudioChunkSource>();
        builder.Services.AddKeyedSingleton<IAudioPipeline>("local", (sp, _) =>
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
                },
                waitForConvergence: false);
        });

        // Sendspin Client services
        builder.Services.AddSingleton<ConnectionOptions>(_ => new ConnectionOptions
        {
            AutoReconnect = false
        });

        builder.Services.AddSingleton<ClientCapabilities>(sp =>
        {
            var settings = sp.GetRequiredService<SettingsService>();
            return settings.GetSendspinClientCapabilities();
        });

        // Player services
        builder.Services.AddSingleton<ISendspinConnection, SendspinConnection>();
        builder.Services.AddSingleton<ISendspinClient>(sp =>
        {
            return new SendspinClientService(
                sp.GetRequiredService<ILogger<SendspinClientService>>(),
                sp.GetRequiredService<ISendspinConnection>(),
                clockSynchronizer: sp.GetRequiredService<IClockSynchronizer>(),
                capabilities: sp.GetRequiredService<ClientCapabilities>(),
                audioPipeline: sp.GetRequiredKeyedService<IAudioPipeline>("sendspin"));
        });
        builder.Services.AddSingleton<SendspinPlayerService>();
        builder.Services.AddSingleton<IPlayerService>(sp => sp.GetRequiredService<SendspinPlayerService>());
        builder.Services.AddSingleton<IPlayerService>(sp =>
        {
            return new LocalAudioPlayerService(
            sp.GetRequiredService<ILogger<LocalAudioPlayerService>>(),
            sp.GetRequiredKeyedService<IAudioPipeline>("local"),
            sp.GetRequiredService<IAudioRenderer>(),
            sp.GetRequiredService<LocalAudioChunkSource>());
        });
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

        return app;
    }

    private static string ResolveLogDirectory()
    {
#if ANDROID
        var externalFilesDir = Android.App.Application.Context.GetExternalFilesDir(null)?.AbsolutePath;
        if (!string.IsNullOrWhiteSpace(externalFilesDir))
        {
            return Path.Combine(externalFilesDir, "logs");
        }
#endif

#if WINDOWS
        var programDirectory = AppContext.BaseDirectory;
        if (!string.IsNullOrWhiteSpace(programDirectory))
        {
            var programLogs = Path.Combine(programDirectory, "logs");
            try
            {
                Directory.CreateDirectory(programLogs);
                var probePath = Path.Combine(programLogs, ".write-test");
                File.WriteAllText(probePath, "ok");
                File.Delete(probePath);
                return programLogs;
            }
            catch
            {
            }
        }

        var documents = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments);
        if (!string.IsNullOrWhiteSpace(documents))
        {
            var documentLogs = Path.Combine(documents, "mashin", "logs");
            try
            {
                Directory.CreateDirectory(documentLogs);
                var probePath = Path.Combine(documentLogs, ".write-test");
                File.WriteAllText(probePath, "ok");
                File.Delete(probePath);
                return documentLogs;
            }
            catch
            {
            }
        }
#endif

        return Path.Combine(FileSystem.AppDataDirectory, "logs");
    }
}
