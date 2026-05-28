using Android.App;
using Android;
using Android.Content;
using Android.Content.PM;
using Android.OS;

namespace mashin
{
    [Activity(Theme = "@style/Maui.SplashTheme", MainLauncher = true, LaunchMode = LaunchMode.SingleTop, ConfigurationChanges = ConfigChanges.ScreenSize | ConfigChanges.Orientation | ConfigChanges.UiMode | ConfigChanges.ScreenLayout | ConfigChanges.SmallestScreenSize | ConfigChanges.Density)]
    public class MainActivity : MauiAppCompatActivity
    {
        private const int NotificationPermissionRequestCode = 4202;
        private const string PostNotificationsPermission = "android.permission.POST_NOTIFICATIONS";

        protected override void OnCreate(Bundle? savedInstanceState)
        {
            base.OnCreate(savedInstanceState);

            RequestNotificationPermissionIfNeeded();
            StartPlaybackNotificationService();
        }

        private void StartPlaybackNotificationService()
        {
            var intent = new Intent(this, typeof(PlaybackNotificationService));
            intent.SetAction(PlaybackNotificationService.ActionStartOrUpdate);
            StartService(intent);
        }

        #pragma warning disable CA1416
        private void RequestNotificationPermissionIfNeeded()
        {
            if (!OperatingSystem.IsAndroidVersionAtLeast(33))
            {
                return;
            }

            if (!OperatingSystem.IsAndroidVersionAtLeast(23))
            {
                return;
            }

            if (CheckSelfPermission(PostNotificationsPermission) == Permission.Granted)
            {
                return;
            }

            RequestPermissions([PostNotificationsPermission], NotificationPermissionRequestCode);
        }
        #pragma warning restore CA1416
    }
}
