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
    #region Constants

        private const int NotificationPermissionRequestCode = 4202;
        private const string PostNotificationsPermission = "android.permission.POST_NOTIFICATIONS";

    #endregion

    #region Activity Lifecycle

        protected override void OnCreate(Bundle? savedInstanceState)
        {
            base.OnCreate(savedInstanceState);

            RequestNotificationPermissionIfNeeded();
            StartPlaybackNotificationService();
        }

        protected override void OnDestroy()
        {
            // Do not stop the service for minimize/background transitions.
            // Only stop when this activity is actually finishing.
            if (IsFinishing && !IsChangingConfigurations)
            {
                StopPlaybackNotificationService();
            }

            base.OnDestroy();
        }

#endregion

#region Service Commands

        private void StartPlaybackNotificationService()
        {
            var intent = new Intent(this, typeof(PlaybackNotificationService));
            intent.SetAction(PlaybackNotificationService.ActionStartOrUpdate);
            StartService(intent);
        }

        private void StopPlaybackNotificationService()
        {
            // Send an explicit terminate action so the service can cleanly
            // stop playback, remove foreground state, and clear notification.
            var intent = new Intent(this, typeof(PlaybackNotificationService));
            intent.SetAction(PlaybackNotificationService.ActionTerminate);
            StartService(intent);
        }

#endregion

#region Permissions

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

#endregion
    }
}
