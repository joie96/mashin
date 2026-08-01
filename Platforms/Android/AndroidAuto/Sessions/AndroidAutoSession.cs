using Android.Content;
using Android.Media;
using Android.Media.Session;
using AndroidX.Car.App;
using AndroidX.Car.App.Media;
using mashin.Models;
using mashin.Platforms.Android.AndroidAuto.Screens;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Maui;
using MediaMetadataCompat = Android.Support.V4.Media.MediaMetadataCompat;
using MediaSessionCompat = Android.Support.V4.Media.Session.MediaSessionCompat;
using PlaybackStateCompat = Android.Support.V4.Media.Session.PlaybackStateCompat;
using SessionPlaybackState = Android.Media.Session.PlaybackState;
using SessionPlaybackStateCode = Android.Media.Session.PlaybackStateCode;

namespace mashin.Platforms.Android.AndroidAuto.Sessions
{
    public class AndroidAutoSession : Session
    {
        private PlaybackService? _playbackService;
        private MediaSession? _mediaSession;
        private MediaSessionCompat? _mediaSessionCompat;
        private bool _mediaPlaybackTokenRegistered;

        public override Screen OnCreateScreen(Intent intent)
        {
            EnsureMediaPlaybackInitialized();
            return new AndroidAutoHomeScreen(CarContext);
        }

        private void EnsureMediaPlaybackInitialized()
        {
            EnsurePlaybackServiceAttached();
            EnsureMediaSession();
            RegisterMediaPlaybackToken();
            SyncMediaSession();
        }

        private void EnsurePlaybackServiceAttached()
        {
            if (_playbackService != null)
            {
                return;
            }

            var services = IPlatformApplication.Current?.Services;
            var playbackService = services?.GetService<PlaybackService>();
            if (playbackService == null)
            {
                return;
            }

            _playbackService = playbackService;
            _playbackService.PropertyChanged += (_, e) =>
            {
                if (e.PropertyName == nameof(PlaybackService.PlaybackState)
                    || e.PropertyName == nameof(PlaybackService.CurrentQueueItem)
                    || e.PropertyName == nameof(PlaybackService.PositionSeconds)
                    || e.PropertyName == nameof(PlaybackService.DurationSeconds))
                {
                    SyncMediaSession();
                }
            };
        }

        private void EnsureMediaSession()
        {
            if (_mediaSession != null && _mediaSessionCompat != null)
            {
                return;
            }

            if (_mediaSession == null)
            {
                _mediaSession = new MediaSession(CarContext, "mashin-carapp-session");
                _mediaSession.SetCallback(new CarMediaSessionCallback(this));
                _mediaSession.Active = true;
            }

            if (_mediaSessionCompat == null)
            {
                _mediaSessionCompat = new MediaSessionCompat(CarContext, "mashin-carapp-session-compat");
                _mediaSessionCompat.SetCallback(new CarMediaSessionCompatCallback(this));
                _mediaSessionCompat.Active = true;
            }
        }

        private void RegisterMediaPlaybackToken()
        {
            if (_mediaPlaybackTokenRegistered || _mediaSessionCompat == null)
            {
                return;
            }

            var mediaPlaybackManager = CarContext.GetCarService(CarContext.MediaPlaybackService) as MediaPlaybackManager;
            if (mediaPlaybackManager == null)
            {
                return;
            }

            var compatToken = _mediaSessionCompat.SessionToken;
            if (compatToken == null)
            {
                return;
            }

            mediaPlaybackManager.RegisterMediaPlaybackToken(compatToken);
            _mediaPlaybackTokenRegistered = true;
        }

        private void SyncMediaSession()
        {
            var mediaSession = _mediaSession;
            var playback = _playbackService;
            if (mediaSession == null || playback == null)
            {
                return;
            }

            var track = playback.CurrentQueueItem?.MediaItem;
            var state = playback.PlaybackState.State;
            var durationSeconds = Math.Max(0, playback.DurationSeconds);
            var positionSeconds = Math.Clamp(playback.PositionSeconds, 0, durationSeconds > 0 ? durationSeconds : double.MaxValue);
            var positionMs = (long)Math.Max(0, positionSeconds * 1000d);

            var playbackStateBuilder = new SessionPlaybackState.Builder();
            playbackStateBuilder.SetActions(
                SessionPlaybackState.ActionPlay
                | SessionPlaybackState.ActionPause
                | SessionPlaybackState.ActionPlayPause
                | SessionPlaybackState.ActionSkipToNext
                | SessionPlaybackState.ActionSkipToPrevious
                | SessionPlaybackState.ActionStop);
            var playbackSpeed = state is PlayerStateType.Playing ? 1f : 0f;
            playbackStateBuilder.SetState(MapPlaybackState(state), positionMs, playbackSpeed, global::Android.OS.SystemClock.ElapsedRealtime());

            var metadataBuilder = new MediaMetadata.Builder();
            metadataBuilder.PutString(MediaMetadata.MetadataKeyTitle, track?.Name ?? string.Empty);
            metadataBuilder.PutString(MediaMetadata.MetadataKeyArtist, track?.ArtistName ?? string.Empty);
            metadataBuilder.PutString(MediaMetadata.MetadataKeyAlbum, track?.AlbumName ?? string.Empty);
            metadataBuilder.PutLong(MediaMetadata.MetadataKeyDuration, (long)Math.Max(0, durationSeconds * 1000d));

            mediaSession.SetPlaybackState(playbackStateBuilder.Build());
            mediaSession.SetMetadata(metadataBuilder.Build());

            if (_mediaSessionCompat != null)
            {
                var compatPlaybackStateBuilder = new PlaybackStateCompat.Builder();
                compatPlaybackStateBuilder.SetActions(
                    PlaybackStateCompat.ActionPlay
                    | PlaybackStateCompat.ActionPause
                    | PlaybackStateCompat.ActionPlayPause
                    | PlaybackStateCompat.ActionSkipToNext
                    | PlaybackStateCompat.ActionSkipToPrevious
                    | PlaybackStateCompat.ActionStop);
                compatPlaybackStateBuilder.SetState(MapCompatPlaybackState(state), positionMs, playbackSpeed, global::Android.OS.SystemClock.ElapsedRealtime());

                var compatMetadataBuilder = new MediaMetadataCompat.Builder();
                compatMetadataBuilder.PutString(MediaMetadataCompat.MetadataKeyTitle, track?.Name ?? string.Empty);
                compatMetadataBuilder.PutString(MediaMetadataCompat.MetadataKeyArtist, track?.ArtistName ?? string.Empty);
                compatMetadataBuilder.PutString(MediaMetadataCompat.MetadataKeyAlbum, track?.AlbumName ?? string.Empty);
                compatMetadataBuilder.PutLong(MediaMetadataCompat.MetadataKeyDuration, (long)Math.Max(0, durationSeconds * 1000d));

                _mediaSessionCompat.SetPlaybackState(compatPlaybackStateBuilder.Build());
                _mediaSessionCompat.SetMetadata(compatMetadataBuilder.Build());
            }
        }

        private static SessionPlaybackStateCode MapPlaybackState(PlayerStateType state)
        {
            return state switch
            {
                PlayerStateType.Playing => SessionPlaybackStateCode.Playing,
                PlayerStateType.Paused => SessionPlaybackStateCode.Paused,
                PlayerStateType.Buffering => SessionPlaybackStateCode.Buffering,
                PlayerStateType.Idle => SessionPlaybackStateCode.Stopped,
                _ => SessionPlaybackStateCode.None
            };
        }

        private static int MapCompatPlaybackState(PlayerStateType state)
        {
            return state switch
            {
                PlayerStateType.Playing => PlaybackStateCompat.StatePlaying,
                PlayerStateType.Paused => PlaybackStateCompat.StatePaused,
                PlayerStateType.Buffering => PlaybackStateCompat.StateBuffering,
                PlayerStateType.Idle => PlaybackStateCompat.StateStopped,
                _ => PlaybackStateCompat.StateNone
            };
        }

        private sealed class CarMediaSessionCallback : MediaSession.Callback
        {
            private readonly AndroidAutoSession _session;

            public CarMediaSessionCallback(AndroidAutoSession session)
            {
                _session = session;
            }

            public override void OnPlay()
            {
                _session.ExecuteTransportCommand(static playback => playback.TogglePlayPauseAsync());
            }

            public override void OnPause()
            {
                _session.ExecuteTransportCommand(static playback => playback.TogglePlayPauseAsync());
            }

            public override void OnSkipToNext()
            {
                _session.ExecuteTransportCommand(static playback => playback.NextTrackAsync());
            }

            public override void OnSkipToPrevious()
            {
                _session.ExecuteTransportCommand(static playback => playback.PreviousTrackAsync());
            }
        }

        private sealed class CarMediaSessionCompatCallback : MediaSessionCompat.Callback
        {
            private readonly AndroidAutoSession _session;

            public CarMediaSessionCompatCallback(AndroidAutoSession session)
            {
                _session = session;
            }

            public override void OnPlay()
            {
                _session.ExecuteTransportCommand(static playback => playback.TogglePlayPauseAsync());
            }

            public override void OnPause()
            {
                _session.ExecuteTransportCommand(static playback => playback.TogglePlayPauseAsync());
            }

            public override void OnSkipToNext()
            {
                _session.ExecuteTransportCommand(static playback => playback.NextTrackAsync());
            }

            public override void OnSkipToPrevious()
            {
                _session.ExecuteTransportCommand(static playback => playback.PreviousTrackAsync());
            }
        }

        private void ExecuteTransportCommand(Func<PlaybackService, Task> command)
        {
            var playback = _playbackService;
            if (playback == null)
            {
                return;
            }

            _ = command(playback).ContinueWith(
                _ => { },
                TaskContinuationOptions.OnlyOnFaulted);
        }
    }
}
