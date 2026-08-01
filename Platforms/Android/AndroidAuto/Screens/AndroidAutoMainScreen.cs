using AndroidX.Car.App;
using AndroidX.Car.App.Model;
using AndroidX.Core.Graphics.Drawable;
using mashin.Models;
using mashin.Platforms.Android.AndroidAuto.Services;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Maui;
using Action = AndroidX.Car.App.Model.Action;

namespace mashin.Platforms.Android.AndroidAuto.Screens
{
    #region Enums

    public enum OverviewTab
    {
        Home,
        Discover,
        Favorites,
        Playlists
    }

    #endregion

    public class AndroidAutoMainScreen : Screen
    {
        #region Fields

        private OverviewTab _activeTab;
        private readonly IPlaylistService? _playlistService;
        private readonly AndroidAutoMediaImageLoader _mediaImageLoader;
        private bool _playlistRefreshStarted;

        #endregion

        #region Construction

        public AndroidAutoMainScreen(CarContext carContext, OverviewTab activeTab = OverviewTab.Home) : base(carContext)
        {
            _activeTab = activeTab;
            var services = IPlatformApplication.Current?.Services;
            _playlistService = services?.GetService<IPlaylistService>();
            var settingsService = services?.GetService<SettingsService>();
            _mediaImageLoader = new AndroidAutoMediaImageLoader(carContext, settingsService, Invalidate);
            if (_playlistService != null)
            {
                _playlistService.PropertyChanged += (_, e) =>
                {
                    if (e.PropertyName == nameof(IPlaylistService.IsLoading))
                    {
                        Invalidate();
                    }
                };
                _playlistService.Playlists.CollectionChanged += (_, _) => Invalidate();
            }
        }

        #endregion

        #region Template Lifecycle

        public override ITemplate OnGetTemplate()
        {
            var tabContents = new TabContents.Builder(BuildActiveTabTemplate())
                .Build();

            return new TabTemplate.Builder(new OverviewTabCallback(this))
                .SetHeaderAction(Action.AppIcon)
                .AddTab(BuildTab(OverviewTab.Home, "Home", Resource.Drawable.home))
                .AddTab(BuildTab(OverviewTab.Discover, "Entdecken", Resource.Drawable.explore))
                .AddTab(BuildTab(OverviewTab.Playlists, "Playlists", Resource.Drawable.playlist_play))
                .AddTab(BuildTab(OverviewTab.Favorites, "Favoriten", Resource.Drawable.favorite))
                .SetActiveTabContentId(GetTabId(_activeTab))
                .SetTabContents(tabContents)
                .Build();
        }

        internal void SetActiveTab(OverviewTab tab)
        {
            if (_activeTab == tab)
            {
                return;
            }

            _activeTab = tab;
            Invalidate();
        }

        #endregion

        #region Tab Template Builders

        private AndroidX.Car.App.Model.Tab BuildTab(OverviewTab tab, string title, int iconResource)
        {
            return new AndroidX.Car.App.Model.Tab.Builder()
                .SetContentId(GetTabId(tab))
                .SetTitle(title)
                .SetIcon(new CarIcon.Builder(IconCompat.CreateWithResource(CarContext, iconResource)).Build())
                .Build();
        }

        private ITemplate BuildActiveTabTemplate()
        {
            if (_activeTab == OverviewTab.Playlists)
            {
                return BuildPlaylistsTabTemplate();
            }

            var itemList = new ItemList.Builder()
                .AddItem(
                    new Row.Builder()
                        .SetTitle(GetTabTitle(_activeTab))
                        .AddText(GetTabBody(_activeTab))
                        .SetOnClickListener(new NavigateOnClickListener(CarContext, ToScreenTarget(_activeTab)))
                        .Build())
                .Build();

            return new ListTemplate.Builder()
                .SetHeaderAction(Action.AppIcon)
                .SetTitle(GetTabTitle(_activeTab))
                .SetSingleList(itemList)
                .AddAction(BuildFabAction(Resource.Drawable.equalizer, ScreenTarget.Player, CarColor.Green))
                .AddAction(BuildFabAction(Resource.Drawable.search, ScreenTarget.Search, CarColor.Blue))
                .Build();
        }

            #endregion

            #region Playlist Tab

        private ITemplate BuildPlaylistsTabTemplate()
        {
            var playlistService = _playlistService;
            if (playlistService == null)
            {
                return new MessageTemplate.Builder("Playlist-Service nicht verfuegbar.")
                    .SetHeaderAction(Action.AppIcon)
                    .SetTitle("Playlists")
                    .Build();
            }

            if (!_playlistRefreshStarted && !playlistService.IsLoading && playlistService.Playlists.Count == 0)
            {
                _playlistRefreshStarted = true;
                _ = LoadPlaylistsAsync();
            }

            if (playlistService.IsLoading)
            {
                return new GridTemplate.Builder()
                    .SetLoading(true)
                    .SetHeaderAction(Action.AppIcon)
                    .SetTitle("Playlists")
                    .AddAction(BuildFabAction(Resource.Drawable.equalizer, ScreenTarget.Player, CarColor.Green))
                    .AddAction(BuildFabAction(Resource.Drawable.search, ScreenTarget.Search, CarColor.Blue))
                    .Build();
            }

            if (playlistService.Playlists.Count == 0)
            {
                return new MessageTemplate.Builder("Keine Playlists verfuegbar.")
                    .SetHeaderAction(Action.AppIcon)
                    .SetTitle("Playlists")
                    .AddAction(BuildFabAction(Resource.Drawable.equalizer, ScreenTarget.Player, CarColor.Green))
                    .AddAction(BuildFabAction(Resource.Drawable.search, ScreenTarget.Search, CarColor.Blue))
                    .Build();
            }

            var itemListBuilder = new ItemList.Builder();
            foreach (var playlist in playlistService.Playlists)
            {
                itemListBuilder.AddItem(BuildPlaylistGridItem(playlist));
            }

            return new GridTemplate.Builder()
                .SetHeaderAction(Action.AppIcon)
                .SetTitle("Playlists")
                .SetItemSize(GridTemplate.ItemSizeLarge)
                .SetItemImageShape(GridTemplate.ItemImageShapeUnset)
                .SetSingleList(itemListBuilder.Build())
                .AddAction(BuildFabAction(Resource.Drawable.equalizer, ScreenTarget.Player, CarColor.Green))
                .AddAction(BuildFabAction(Resource.Drawable.search, ScreenTarget.Search, CarColor.Blue))
                .Build();
        }

        private GridItem BuildPlaylistGridItem(Playlist playlist)
        {
            var imageIcon = CreatePlaylistIcon(playlist.ImageUri);

            return new GridItem.Builder()
                .SetImage(imageIcon, GridItem.ImageTypeLarge)
                .SetTitle(playlist.DisplayName)
                .SetOnClickListener(new OpenPlaylistDetailOnClickListener(CarContext, playlist))
                .Build();
        }

        private CarIcon CreatePlaylistIcon(string? imageUri)
        {
            return _mediaImageLoader.GetImageIconOrPlaceholder(imageUri, Resource.Drawable.playlist_play);
        }

        private async Task LoadPlaylistsAsync()
        {
            var playlistService = _playlistService;
            if (playlistService == null)
            {
                return;
            }

            await playlistService.RefreshAsync();
            Invalidate();
        }

        #endregion

        #region Genrell UI and Navigation Tabs

        private Action BuildFabAction(int iconResource, ScreenTarget target, CarColor backgroundColor)
        {
            return new Action.Builder()
                .SetIcon(new CarIcon.Builder(IconCompat.CreateWithResource(CarContext, iconResource)).Build())
                .SetBackgroundColor(backgroundColor)
                .SetOnClickListener(new NavigateOnClickListener(CarContext, target))
                .Build();
        }

        private static string GetTabId(OverviewTab tab)
        {
            return tab switch
            {
                OverviewTab.Home => "tab_home",
                OverviewTab.Discover => "tab_discover",
                OverviewTab.Favorites => "tab_favorites",
                OverviewTab.Playlists => "tab_playlists",
                _ => "tab_home"
            };
        }

        private static string GetTabTitle(OverviewTab tab)
        {
            return tab switch
            {
                OverviewTab.Home => "Home",
                OverviewTab.Discover => "Entdecken",
                OverviewTab.Favorites => "Favoriten",
                OverviewTab.Playlists => "Playlists",
                _ => "mashin"
            };
        }

        private static string GetTabBody(OverviewTab tab)
        {
            return tab switch
            {
                OverviewTab.Home => "Startpunkt der Android-Auto-Ansicht.",
                OverviewTab.Discover => "Entdecken-Ansicht folgt im nachsten Schritt.",
                OverviewTab.Favorites => "Favoriten-Ansicht folgt im nachsten Schritt.",
                OverviewTab.Playlists => "Playlist-Ansicht folgt im nachsten Schritt.",
                _ => "Inhalte folgen im nachsten Schritt."
            };
        }

        private static OverviewTab ParseTab(string tabId)
        {
            return tabId switch
            {
                "tab_home" => OverviewTab.Home,
                "tab_discover" => OverviewTab.Discover,
                "tab_favorites" => OverviewTab.Favorites,
                "tab_playlists" => OverviewTab.Playlists,
                _ => OverviewTab.Home
            };
        }

        private static ScreenTarget ToScreenTarget(OverviewTab tab)
        {
            return tab switch
            {
                OverviewTab.Home => ScreenTarget.Home,
                OverviewTab.Discover => ScreenTarget.Discover,
                OverviewTab.Favorites => ScreenTarget.Favorites,
                OverviewTab.Playlists => ScreenTarget.Playlists,
                _ => ScreenTarget.Home
            };
        }

        #endregion

        #region Nested Types

        private sealed class OverviewTabCallback : Java.Lang.Object, TabTemplate.ITabCallback
        {
            private readonly AndroidAutoMainScreen _screen;

            public OverviewTabCallback(AndroidAutoMainScreen screen)
            {
                _screen = screen;
            }

            public void OnTabSelected(string? tabContentId)
            {
                _screen.SetActiveTab(ParseTab(tabContentId ?? string.Empty));
            }
        }

        #endregion
    }

    #region Navigation Targets

    internal enum ScreenTarget
    {
        Home,
        Discover,
        Favorites,
        Playlists,
        Search,
        Player
    }

    #endregion

    #region Navigation Listeners

    internal sealed class NavigateOnClickListener : Java.Lang.Object, IOnClickListener
    {
        private readonly CarContext _carContext;
        private readonly ScreenTarget _screenTarget;

        public NavigateOnClickListener(CarContext carContext, ScreenTarget screenTarget)
        {
            _carContext = carContext;
            _screenTarget = screenTarget;
        }

        public void OnClick()
        {
            var screenManager = (ScreenManager)_carContext.GetCarService(CarContext.ScreenService);
            Screen screen = _screenTarget switch
            {
                ScreenTarget.Player => new AndroidAutoPlaybackScreen(_carContext),
                _ => new AndroidAutoPlaceholderScreen(_carContext, _screenTarget)
            };
            screenManager.Push(screen);
        }
    }

    internal sealed class OpenPlaylistDetailOnClickListener : Java.Lang.Object, IOnClickListener
    {
        private readonly CarContext _carContext;
        private readonly Playlist _playlist;

        public OpenPlaylistDetailOnClickListener(CarContext carContext, Playlist playlist)
        {
            _carContext = carContext;
            _playlist = playlist;
        }

        public void OnClick()
        {
            var screenManager = (ScreenManager)_carContext.GetCarService(CarContext.ScreenService);
            screenManager.Push(new AndroidAutoPlaylistDetailScreen(_carContext, _playlist));
        }
    }

    #endregion

    #region Placeholder Screen

    internal sealed class AndroidAutoPlaceholderScreen : Screen
    {
        private readonly ScreenTarget _screenTarget;

        public AndroidAutoPlaceholderScreen(CarContext carContext, ScreenTarget screenTarget) : base(carContext)
        {
            _screenTarget = screenTarget;
        }

        public override ITemplate OnGetTemplate()
        {
            var title = _screenTarget switch
            {
                ScreenTarget.Home => "Home",
                ScreenTarget.Discover => "Entdecken",
                ScreenTarget.Favorites => "Favoriten",
                ScreenTarget.Playlists => "Playlists",
                ScreenTarget.Search => "Suche",
                ScreenTarget.Player => "Playerubersicht",
                _ => "mashin"
            };

            var body = _screenTarget switch
            {
                ScreenTarget.Search => "Hier folgt die Suchansicht.",
                ScreenTarget.Player => "Hier folgt die Playerubersicht.",
                _ => "Inhalte folgen im nachsten Schritt."
            };

            return new MessageTemplate.Builder(body)
                .SetHeaderAction(Action.Back)
                .SetTitle(title)
                .Build();
        }
    }

    #endregion
}
