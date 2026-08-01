using Android.Content;
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

namespace mashin.Platforms.Android.AndroidAuto.Sessions
{
    public class AndroidAutoSession : Session
    {
        private PlaybackService? _playbackService;
        private IConnectionService? _connectionService;
        private MediaSessionCompat? _mediaSessionCompat;
        private bool _mediaPlaybackTokenRegistered;
        private bool _playbackInitStarted;
        private bool _connectionConnectStarted;

        public override Screen OnCreateScreen(Intent intent)
        {
            EnsureMediaPlaybackInitialized();
            return new AndroidAutoMainScreen(CarContext);
        }

        private void EnsureMediaPlaybackInitialized()
        {
            EnsureConnectionServiceAttached();
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

            if (!_playbackInitStarted)
            {
                _playbackInitStarted = true;
                _ = Task.Run(async () =>
                {
                    try
                    {
                        await playbackService.InitializeAsync();
                        await playbackService.SetOutputModeAsync(PlaybackOutputMode.Sendspin);
                    }
                    catch
                    {
                        _playbackInitStarted = false;
                    }
                });
            }
        }

        private void EnsureConnectionServiceAttached()
        {
            if (_connectionService != null)
            {
                return;
            }

            var services = IPlatformApplication.Current?.Services;
            _connectionService = services?.GetService<IConnectionService>();

            if (_connectionService != null && !_connectionConnectStarted)
            {
                _connectionConnectStarted = true;
                _ = Task.Run(async () =>
                {
                    try
                    {
                        await _connectionService.ConnectAsync();
                    }
                    catch
                    {
                        _connectionConnectStarted = false;
                    }
                });
            }
        }

        private void EnsureMediaSession()
        {
            if (_mediaSessionCompat != null)
            {
                return;
            }

            _mediaSessionCompat = new MediaSessionCompat(CarContext, "mashin-carapp-session-compat");
            _mediaSessionCompat.SetCallback(new CarMediaSessionCompatCallback(this));
            _mediaSessionCompat.Active = true;
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
            var mediaSessionCompat = _mediaSessionCompat;
            var playback = _playbackService;
            if (mediaSessionCompat == null || playback == null)
            {
                return;
            }

            var track = playback.CurrentQueueItem?.MediaItem;
            var state = playback.PlaybackState.State;
            var durationSeconds = Math.Max(0, playback.DurationSeconds);
            var positionSeconds = Math.Clamp(playback.PositionSeconds, 0, durationSeconds > 0 ? durationSeconds : double.MaxValue);
            var positionMs = (long)Math.Max(0, positionSeconds * 1000d);
            var playbackSpeed = state is PlayerStateType.Playing ? 1f : 0f;

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

            mediaSessionCompat.SetPlaybackState(compatPlaybackStateBuilder.Build());
            mediaSessionCompat.SetMetadata(compatMetadataBuilder.Build());
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
