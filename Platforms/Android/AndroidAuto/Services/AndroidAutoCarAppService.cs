using Android.App;
using Android.Content;
using AndroidX.Car.App;
using AndroidX.Car.App.Validation;
using mashin.Platforms.Android.AndroidAuto.Sessions;

namespace mashin.Platforms.Android.AndroidAuto.Services
{
    [Service(Exported = true)]
    [IntentFilter(new[] { "androidx.car.app.CarAppService" }, Categories = new[] { "androidx.car.app.category.POI" })]
    public class AndroidAutoCarAppService : CarAppService
    {
        public override HostValidator CreateHostValidator()
        {
            return HostValidator.AllowAllHostsValidator;
        }

        public override Session OnCreateSession()
        {
            return new AndroidAutoSession();
        }
    }
}
