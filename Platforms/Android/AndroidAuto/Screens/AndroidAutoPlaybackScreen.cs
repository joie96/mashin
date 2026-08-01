using AndroidX.Car.App;
using AndroidX.Car.App.Media.Model;
using AndroidX.Car.App.Model;
using AndroidX.Core.Graphics.Drawable;
using Action = AndroidX.Car.App.Model.Action;

namespace mashin.Platforms.Android.AndroidAuto.Screens
{
    internal sealed class AndroidAutoPlaybackScreen : Screen
    {
        public AndroidAutoPlaybackScreen(CarContext carContext) : base(carContext)
        {
        }

        public override ITemplate OnGetTemplate()
        {
            var header = new Header.Builder()
                .SetStartHeaderAction(Action.Back)
                .SetTitle("Wiedergabe")
                .AddEndHeaderAction(
                    new Action.Builder()
                        .SetIcon(new CarIcon.Builder(IconCompat.CreateWithResource(CarContext, Resource.Drawable.playlist_play)).Build())
                        .SetOnClickListener(new NavigateOnClickListener(CarContext, ScreenTarget.Playlists))
                        .Build())
                .Build();

            return new MediaPlaybackTemplate.Builder()
                .SetHeader(header)
                .Build();
        }
    }
}
