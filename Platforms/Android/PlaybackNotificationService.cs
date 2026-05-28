using Android;
using Android.App;
using Android.Content;
using Android.Media;
using Android.Media.Session;
using Android.OS;
using mashin.Models;
using mashin.Services;
using Microsoft.Maui;
using Microsoft.Extensions.DependencyInjection;
using System.ComponentModel;
using SessionPlaybackState = Android.Media.Session.PlaybackState;
using SessionPlaybackStateCode = Android.Media.Session.PlaybackStateCode;

namespace mashin;

[Service(Enabled = true, Exported = false)]
public sealed class PlaybackNotificationService : Service
{
    private const string ChannelId = "mashin.playback";
    private const int NotificationId = 4201;
    private const PendingIntentFlags ImmutableCompatFlag = (PendingIntentFlags)0x04000000;

    public const string ActionStartOrUpdate = "mashin.action.START_OR_UPDATE";
    public const string ActionPlayPause = "mashin.action.PLAY_PAUSE";
    public const string ActionNext = "mashin.action.NEXT";
    public const string ActionPrevious = "mashin.action.PREVIOUS";
    public const string ActionStop = "mashin.action.STOP";

    private IPlaybackService? _playbackService;
    private MediaSession? _mediaSession;
    private NotificationManager? _notificationManager;
    private bool _isForeground;

    public override void OnCreate()
    {
        base.OnCreate();

        _notificationManager = GetSystemService(NotificationService) as NotificationManager;
        CreateNotificationChannel();

        _mediaSession = new MediaSession(this, "mashin-playback-session");
        _mediaSession.SetCallback(new MediaSessionCallback(this));
        _mediaSession.Active = true;

        TryAttachPlaybackService();
    }

    public override StartCommandResult OnStartCommand(Intent? intent, StartCommandFlags flags, int startId)
    {
        _ = HandleIntentAsync(intent);
        return StartCommandResult.Sticky;
    }

    public override IBinder? OnBind(Intent? intent) => null;

    public override void OnDestroy()
    {
        if (_playbackService != null)
        {
            _playbackService.CurrentTrackUpdated -= OnPlaybackSourceChanged;
            _playbackService.CurrentPlayerQueueUpdated -= OnPlaybackSourceChanged;
            _playbackService.PropertyChanged -= OnPlaybackPropertyChanged;
        }

        if (_mediaSession != null)
        {
            _mediaSession.Active = false;
            _mediaSession.Release();
            _mediaSession.Dispose();
            _mediaSession = null;
        }

        base.OnDestroy();
    }

    private async Task HandleIntentAsync(Intent? intent)
    {
        TryAttachPlaybackService();
        var playback = _playbackService;
        if (playback == null)
        {
            return;
        }

        var action = intent?.Action;

        try
        {
            switch (action)
            {
                case ActionPlayPause:
                    await playback.TogglePlayPauseAsync();
                    break;
                case ActionNext:
                    await playback.NextTrackAsync();
                    break;
                case ActionPrevious:
                    await playback.PreviousTrackAsync();
                    break;
                case ActionStop:
                    await playback.StopAsync();
                    break;
            }

            if (!string.IsNullOrWhiteSpace(action))
            {
                await playback.RefreshNowAsync();
            }
        }
        catch
        {
            // Keep service alive even if remote command fails.
        }

        UpdateNotification();
    }

    private void OnPlaybackSourceChanged(object? sender, EventArgs e)
    {
        UpdateNotification();
    }

    private void OnPlaybackPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(IPlaybackService.PlaybackState)
            || e.PropertyName == nameof(IPlaybackService.CurrentTrack)
            || e.PropertyName == nameof(IPlaybackService.CurrentPlayerQueue))
        {
            UpdateNotification();
        }
    }

    private void UpdateNotification()
    {
        TryAttachPlaybackService();
        var playback = _playbackService;
        if (playback == null)
        {
            return;
        }

        var track = playback.CurrentTrack;
        var state = playback.PlaybackState.State;
        var hasTrack = track != null;

        var shouldShow = hasTrack || state is PlayerPlaybackState.Playing or PlayerPlaybackState.Buffering or PlayerPlaybackState.Paused or PlayerPlaybackState.Seeking;

        if (!shouldShow)
        {
            if (_isForeground)
            {
                if (OperatingSystem.IsAndroidVersionAtLeast(24))
                {
                    StopForeground(StopForegroundFlags.Remove);
                }
                else
                {
                    StopForeground(true);
                }

                _isForeground = false;
            }

            StopSelf();
            return;
        }

        UpdateMediaSession(track, state);

        var notification = BuildNotification(track, state);
        if (!_isForeground)
        {
            StartForeground(NotificationId, notification);
            _isForeground = true;
            return;
        }

        _notificationManager?.Notify(NotificationId, notification);
    }

    #pragma warning disable CA1422
    private Notification BuildNotification(Track? track, PlayerPlaybackState state)
    {
        var immutableFlag = Build.VERSION.SdkInt >= BuildVersionCodes.M
            ? ImmutableCompatFlag
            : 0;

        var contentIntent = PendingIntent.GetActivity(
            this,
            100,
            new Intent(this, typeof(MainActivity))
                .SetFlags(ActivityFlags.SingleTop | ActivityFlags.ClearTop),
            immutableFlag | PendingIntentFlags.UpdateCurrent);

        var previousIntent = CreateActionIntent(ActionPrevious, 101);
        var playPauseIntent = CreateActionIntent(ActionPlayPause, 102);
        var nextIntent = CreateActionIntent(ActionNext, 103);
        var stopIntent = CreateActionIntent(ActionStop, 104);

        var isPlaying = state is PlayerPlaybackState.Playing or PlayerPlaybackState.Seeking or PlayerPlaybackState.Buffering;
        var playPauseIcon = isPlaying ? Android.Resource.Drawable.IcMediaPause : Android.Resource.Drawable.IcMediaPlay;
        var playPauseLabel = isPlaying ? "Pause" : "Play";

        Notification.Builder builder;
        if (OperatingSystem.IsAndroidVersionAtLeast(26))
        {
            builder = new Notification.Builder(this, ChannelId);
        }
        else
        {
            builder = new Notification.Builder(this);
        }

        builder.SetSmallIcon(Resource.Mipmap.appicon);
        builder.SetVisibility(NotificationVisibility.Public);
        builder.SetOnlyAlertOnce(true);
        builder.SetContentTitle(track?.Name ?? "mashin");
        builder.SetContentText(track?.ArtistName ?? "Wiedergabe");
        builder.SetSubText(track?.AlbumName);
        builder.SetContentIntent(contentIntent);
        builder.SetDeleteIntent(stopIntent);
        builder.SetOngoing(isPlaying);

#pragma warning disable CS0618
#pragma warning disable CA1422
        builder.AddAction(Android.Resource.Drawable.IcMediaPrevious, "Zurueck", previousIntent);
        builder.AddAction(playPauseIcon, playPauseLabel, playPauseIntent);
        builder.AddAction(Android.Resource.Drawable.IcMediaNext, "Weiter", nextIntent);
#pragma warning restore CA1422
#pragma warning restore CS0618

        var mediaSession = _mediaSession;
        var sessionToken = mediaSession?.SessionToken;
        if (sessionToken != null)
        {
            var style = new Notification.MediaStyle();
            style.SetMediaSession(sessionToken);
            style.SetShowActionsInCompactView(0, 1, 2);
            builder.SetStyle(style);
        }

        return builder.Build()!;
    }
    #pragma warning restore CA1422

    private void UpdateMediaSession(Track? track, PlayerPlaybackState state)
    {
        var mediaSession = _mediaSession;
        if (mediaSession == null)
        {
            return;
        }

        var playbackStateBuilder = new SessionPlaybackState.Builder();
        playbackStateBuilder.SetActions(
            SessionPlaybackState.ActionPlay
            | SessionPlaybackState.ActionPause
            | SessionPlaybackState.ActionPlayPause
            | SessionPlaybackState.ActionSkipToNext
            | SessionPlaybackState.ActionSkipToPrevious
            | SessionPlaybackState.ActionStop);
        playbackStateBuilder.SetState(MapPlaybackState(state), SessionPlaybackState.PlaybackPositionUnknown, 1f);
        var playbackState = playbackStateBuilder.Build();

        var metadataBuilder = new MediaMetadata.Builder();
        metadataBuilder.PutString(MediaMetadata.MetadataKeyTitle, track?.Name ?? string.Empty);
        metadataBuilder.PutString(MediaMetadata.MetadataKeyArtist, track?.ArtistName ?? string.Empty);
        metadataBuilder.PutString(MediaMetadata.MetadataKeyAlbum, track?.AlbumName ?? string.Empty);

        mediaSession.SetPlaybackState(playbackState!);
        mediaSession.SetMetadata(metadataBuilder.Build()!);
    }

    private static SessionPlaybackStateCode MapPlaybackState(PlayerPlaybackState state)
    {
        return state switch
        {
            PlayerPlaybackState.Playing => SessionPlaybackStateCode.Playing,
            PlayerPlaybackState.Paused => SessionPlaybackStateCode.Paused,
            PlayerPlaybackState.Buffering => SessionPlaybackStateCode.Buffering,
            PlayerPlaybackState.Seeking => SessionPlaybackStateCode.FastForwarding,
            PlayerPlaybackState.Stopped => SessionPlaybackStateCode.Stopped,
            _ => SessionPlaybackStateCode.None
        };
    }

    private PendingIntent CreateActionIntent(string action, int requestCode)
    {
        var immutableFlag = Build.VERSION.SdkInt >= BuildVersionCodes.M
            ? ImmutableCompatFlag
            : 0;

        var intent = new Intent(this, typeof(PlaybackNotificationService));
        intent.SetAction(action);

        return PendingIntent.GetService(
            this,
            requestCode,
            intent,
            immutableFlag | PendingIntentFlags.UpdateCurrent)!;
    }

    private void CreateNotificationChannel()
    {
        if (!OperatingSystem.IsAndroidVersionAtLeast(26))
        {
            return;
        }

        if (_notificationManager == null)
        {
            return;
        }

        var existingChannel = _notificationManager.GetNotificationChannel(ChannelId);
        if (existingChannel != null)
        {
            return;
        }

        var channel = new NotificationChannel(
            ChannelId,
            "Playback",
            NotificationImportance.Low);

        channel.Description = "Steuert die Medienwiedergabe";
        channel.SetShowBadge(false);
        _notificationManager.CreateNotificationChannel(channel);
    }

    private void TryAttachPlaybackService()
    {
        if (_playbackService != null)
        {
            return;
        }

        var services = IPlatformApplication.Current?.Services;
        var playbackService = services?.GetService<IPlaybackService>();
        if (playbackService == null)
        {
            return;
        }

        _playbackService = playbackService;
        _playbackService.CurrentTrackUpdated += OnPlaybackSourceChanged;
        _playbackService.CurrentPlayerQueueUpdated += OnPlaybackSourceChanged;
        _playbackService.PropertyChanged += OnPlaybackPropertyChanged;
    }

    private sealed class MediaSessionCallback : MediaSession.Callback
    {
        private readonly PlaybackNotificationService _service;

        public MediaSessionCallback(PlaybackNotificationService service)
        {
            _service = service;
        }

        public override void OnPlay()
        {
            _ = _service.HandleIntentAsync(new Intent().SetAction(ActionPlayPause));
        }

        public override void OnPause()
        {
            _ = _service.HandleIntentAsync(new Intent().SetAction(ActionPlayPause));
        }

        public override void OnSkipToNext()
        {
            _ = _service.HandleIntentAsync(new Intent().SetAction(ActionNext));
        }

        public override void OnSkipToPrevious()
        {
            _ = _service.HandleIntentAsync(new Intent().SetAction(ActionPrevious));
        }

        public override void OnStop()
        {
            _ = _service.HandleIntentAsync(new Intent().SetAction(ActionStop));
        }
    }
}
