using mashin.Services;
using mashin.Views.Desktop;
using DesktopMainPage = mashin.Views.Desktop.MainPage;
using MobileMainPage = mashin.Views.Mobile.MainPage;
using Microsoft.Extensions.DependencyInjection;
using FFImageLoading;
using FFImageLoading.Config;
using mashin.Resources.Styles;

namespace mashin;

public partial class App : Application
{
    private readonly SettingsService _settings;
    private static bool _ffImageLoadingConfigured;

    public App(SettingsService settings)
    {
        InitializeComponent();

        _settings = settings;
        SetTheme(_settings.ThemeMode);
    }

    public void SetTheme(AppTheme theme)
    {
        if (Resources == null)
        {
            return;
        }

        var dictionaries = Resources.MergedDictionaries;
        var existingThemes = dictionaries
            .Where(dictionary => dictionary is DarkTheme || dictionary is LightTheme)
            .ToList();

        foreach (var existingTheme in existingThemes)
        {
            dictionaries.Remove(existingTheme);
        }

        var resolvedTheme = theme == AppTheme.Unspecified
            ? (Application.Current?.RequestedTheme ?? AppTheme.Dark)
            : theme;
        ResourceDictionary nextThemeDictionary = resolvedTheme == AppTheme.Dark
            ? new DarkTheme()
            : new LightTheme();

        dictionaries.Add(nextThemeDictionary);

        UserAppTheme = theme;
    }

    protected override Window CreateWindow(IActivationState? activationState)
    {
        // Configure FFImageLoading once at app startup
        if (!_ffImageLoadingConfigured)
        {
            try
            {
                ImageService.Instance.Initialize(new Configuration
                {
                    MaxMemoryCacheSize = 64 * 1024 * 1024
                });
                _ffImageLoadingConfigured = true;
            }
            catch
            {
            }
        }

        var services = Handler!.MauiContext!.Services;

        Page mainPage;
        if (DeviceInfo.Current.Platform == DevicePlatform.Android)
        {
            mainPage = services.GetRequiredService<MobileMainPage>();
        }
        else
        {
            mainPage = services.GetRequiredService<DesktopMainPage>();
        }
        
        var window = new Window(mainPage);

        return window;
    }
}