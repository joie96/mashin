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
    internal sealed class AndroidAutoPlaylistDetailScreen : Screen
    {
        private readonly Playlist _playlist;
        private readonly MusicAssistantService? _musicAssistantService;
        private readonly PlaybackService? _playbackService;
        private readonly AndroidAutoMediaImageLoader _mediaImageLoader;

        private readonly List<Track> _tracks = new();
        private bool _loadStarted;
        private bool _isLoading;

        public AndroidAutoPlaylistDetailScreen(CarContext carContext, Playlist playlist) : base(carContext)
        {
            _playlist = playlist;

            var services = IPlatformApplication.Current?.Services;
            _musicAssistantService = services?.GetService<MusicAssistantService>();
            _playbackService = services?.GetService<PlaybackService>();
            var settingsService = services?.GetService<SettingsService>();
            _mediaImageLoader = new AndroidAutoMediaImageLoader(carContext, settingsService, Invalidate);
        }

        public override ITemplate OnGetTemplate()
        {
            if (!_loadStarted)
            {
                _loadStarted = true;
                _isLoading = true;
                _ = LoadTracksAsync();
            }

            var itemListBuilder = new ItemList.Builder()
                .AddItem(BuildPlayActionRow())
                .AddItem(BuildShuffleActionRow());

            if (_isLoading)
            {
                itemListBuilder.AddItem(new Row.Builder().SetTitle("Titel werden geladen...").Build());
            }
            else if (_tracks.Count == 0)
            {
                itemListBuilder.AddItem(new Row.Builder().SetTitle("Keine Titel in dieser Playlist.").Build());
            }
            else
            {
                foreach (var track in _tracks)
                {
                    itemListBuilder.AddItem(BuildTrackRow(track));
                }
            }

            return new ListTemplate.Builder()
                .SetHeaderAction(Action.Back)
                .SetTitle(GetPlaylistTitle())
                .SetSingleList(itemListBuilder.Build())
                .Build();
        }

        private Row BuildPlayActionRow()
        {
            return new Row.Builder()
                .SetTitle("Abspielen")
                .SetImage(new CarIcon.Builder(IconCompat.CreateWithResource(CarContext, Resource.Drawable.play)).Build(), Row.ImageTypeIcon)
                .SetOnClickListener(new AsyncOnClickListener(async () =>
                {
                    var playbackService = _playbackService;
                    if (playbackService == null)
                    {
                        return;
                    }

                    await playbackService.PlayMediaAsync(new List<MediaItem> { _playlist });
                }))
                .Build();
        }

        private Row BuildShuffleActionRow()
        {
            return new Row.Builder()
                .SetTitle("Zufallige Wiedergabe")
                .SetImage(new CarIcon.Builder(IconCompat.CreateWithResource(CarContext, Resource.Drawable.shuffle)).Build(), Row.ImageTypeIcon)
                .SetOnClickListener(new AsyncOnClickListener(async () =>
                {
                    var playbackService = _playbackService;
                    if (playbackService == null)
                    {
                        return;
                    }

                    if (_tracks.Count > 0)
                    {
                        await playbackService.ShufflePlayMediaAsync(_tracks.Cast<MediaItem>().ToList());
                        return;
                    }

                    await playbackService.PlayMediaAsync(new List<MediaItem> { _playlist });
                }))
                .Build();
        }

        private Row BuildTrackRow(Track track)
        {
            var artistText = GetArtistText(track);
            var coverUri = track.ImageUri ?? track.Album?.ImageUri;
            var cover = _mediaImageLoader.GetImageIconOrPlaceholder(coverUri, Resource.Drawable.playlist_play);

            var rowBuilder = new Row.Builder()
                .SetTitle(GetTrackTitle(track))
                .SetImage(cover, Row.ImageTypeSmall)
                .SetOnClickListener(new AsyncOnClickListener(async () =>
                {
                    var playbackService = _playbackService;
                    if (playbackService == null)
                    {
                        return;
                    }

                    await playbackService.PlayMediaAsync(new List<MediaItem> { track });
                }));

            if (!string.IsNullOrWhiteSpace(artistText))
            {
                rowBuilder.AddText(artistText);
            }

            return rowBuilder.Build();
        }

        private async Task LoadTracksAsync()
        {
            try
            {
                var musicAssistantService = _musicAssistantService;
                if (musicAssistantService == null
                    || string.IsNullOrWhiteSpace(_playlist.ItemId)
                    || string.IsNullOrWhiteSpace(_playlist.Provider))
                {
                    _tracks.Clear();
                    return;
                }

                var tracks = await musicAssistantService.GetPlaylistTracksAsync(
                    _playlist.ItemId,
                    _playlist.Provider,
                    forceRefresh: true);

                _tracks.Clear();
                _tracks.AddRange(tracks ?? Enumerable.Empty<Track>());
            }
            catch
            {
                _tracks.Clear();
            }
            finally
            {
                _isLoading = false;
                Invalidate();
            }
        }

        private string GetPlaylistTitle()
        {
            if (!string.IsNullOrWhiteSpace(_playlist.DisplayName))
            {
                return _playlist.DisplayName;
            }

            if (!string.IsNullOrWhiteSpace(_playlist.Name))
            {
                return _playlist.Name;
            }

            return "Playlist";
        }

        private static string GetTrackTitle(Track track)
        {
            if (!string.IsNullOrWhiteSpace(track.DisplayName))
            {
                return track.DisplayName;
            }

            if (!string.IsNullOrWhiteSpace(track.Name))
            {
                return track.Name;
            }

            return "Unbekannter Titel";
        }

        private static string GetArtistText(Track track)
        {
            if (track.Artists == null || track.Artists.Count == 0)
            {
                return string.Empty;
            }

            var names = track.Artists
                .Select(artist => artist?.DisplayName)
                .Where(name => !string.IsNullOrWhiteSpace(name))
                .ToList();

            return names.Count == 0 ? string.Empty : string.Join(", ", names);
        }
    }

    internal sealed class AsyncOnClickListener : Java.Lang.Object, IOnClickListener
    {
        private readonly Func<Task> _action;

        public AsyncOnClickListener(Func<Task> action)
        {
            _action = action;
        }

        public void OnClick()
        {
            _ = ExecuteAsync();
        }

        private async Task ExecuteAsync()
        {
            try
            {
                await _action();
            }
            catch
            {
                // Ignore action errors to avoid destabilizing the host callback thread.
            }
        }
    }
}
