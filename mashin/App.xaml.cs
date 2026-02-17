using mashin.Services;
using mashin.Views.Desktop;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace mashin;

public partial class App : Application
{
    private readonly SettingsService _settings;

    public App(SettingsService settings)
    {
        InitializeComponent();

        _settings = settings;
    }

    public void SetTheme(AppTheme theme)
    {
        var dictionaries = Resources.MergedDictionaries;
        var themeSources = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
        {
            "Resources/Styles/DarkTheme.xaml",
            "Resources/Styles/LightTheme.xaml"
        };

        foreach (var dictionary in dictionaries.ToList())
        {
            var source = dictionary.Source?.OriginalString;
            if (!string.IsNullOrWhiteSpace(source) && themeSources.Contains(source))
            {
                dictionaries.Remove(dictionary);
            }
        }

        var resolvedTheme = theme == AppTheme.Unspecified ? RequestedTheme : theme;
        var nextSource = resolvedTheme == AppTheme.Dark
            ? "Resources/Styles/DarkTheme.xaml"
            : "Resources/Styles/LightTheme.xaml";

        if (!dictionaries.Any(dictionary =>
                string.Equals(dictionary.Source?.OriginalString, nextSource, StringComparison.OrdinalIgnoreCase)))
        {
            dictionaries.Add(new ResourceDictionary { Source = new Uri(nextSource, UriKind.Relative) });
        }

        UserAppTheme = theme;
    }

    protected override Window CreateWindow(IActivationState? activationState)
    {
        var services = Handler!.MauiContext!.Services;

        // MainPage als Window
        var mainPage = services.GetRequiredService<MainPage>();
        //var favoritesCache = services.GetRequiredService<FavoritesCacheService>();
        //_ = favoritesCache.InitializeAsync();
        var window = new Window(mainPage);

        return window;
    }
}