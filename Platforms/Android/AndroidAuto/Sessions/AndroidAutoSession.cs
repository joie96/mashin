using Android.Content;
using AndroidX.Car.App;
using mashin.Platforms.Android.AndroidAuto.Screens;

namespace mashin.Platforms.Android.AndroidAuto.Sessions
{
    public class AndroidAutoSession : Session
    {
        public override Screen OnCreateScreen(Intent intent)
        {
            return new AndroidAutoHomeScreen(CarContext);
        }
    }
}
