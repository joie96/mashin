using AndroidX.Car.App;
using AndroidX.Car.App.Model;
using AndroidX.Core.Graphics.Drawable;
using Action = AndroidX.Car.App.Model.Action;

namespace mashin.Platforms.Android.AndroidAuto.Screens
{
    public enum OverviewTab
    {
        Home,
        Discover,
        Favorites,
        Playlists
    }

    public class AndroidAutoHomeScreen : Screen
    {
        private OverviewTab _activeTab;

        public AndroidAutoHomeScreen(CarContext carContext, OverviewTab activeTab = OverviewTab.Home) : base(carContext)
        {
            _activeTab = activeTab;
        }

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

        private sealed class OverviewTabCallback : Java.Lang.Object, TabTemplate.ITabCallback
        {
            private readonly AndroidAutoHomeScreen _screen;

            public OverviewTabCallback(AndroidAutoHomeScreen screen)
            {
                _screen = screen;
            }

            public void OnTabSelected(string? tabContentId)
            {
                _screen.SetActiveTab(ParseTab(tabContentId ?? string.Empty));
            }
        }
    }

    internal enum ScreenTarget
    {
        Home,
        Discover,
        Favorites,
        Playlists,
        Search,
        Player
    }

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
            Screen screen = _screenTarget == ScreenTarget.Player
                ? new AndroidAutoPlaybackScreen(_carContext)
                : new AndroidAutoPlaceholderScreen(_carContext, _screenTarget);
            screenManager.Push(screen);
        }
    }

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
}
