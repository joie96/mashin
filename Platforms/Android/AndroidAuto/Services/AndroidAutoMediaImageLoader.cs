using Android.Graphics;
using AndroidX.Car.App;
using AndroidX.Car.App.Model;
using AndroidX.Core.Graphics.Drawable;
using mashin.Services;
using Microsoft.Maui.ApplicationModel;
using System.Net.Http;

namespace mashin.Platforms.Android.AndroidAuto.Services
{
    internal sealed class AndroidAutoMediaImageLoader
    {
        private static readonly HttpClient RemoteImageHttpClient = new();
        private static readonly Dictionary<string, CarIcon> SharedIconCache = new(StringComparer.Ordinal);
        private static readonly HashSet<string> SharedLoadingUris = new(StringComparer.Ordinal);
        private static readonly object SharedLock = new();

        private readonly CarContext _carContext;
        private readonly SettingsService? _settingsService;
        private readonly global::System.Action _invalidate;

        public AndroidAutoMediaImageLoader(CarContext carContext, SettingsService? settingsService, global::System.Action invalidate)
        {
            _carContext = carContext;
            _settingsService = settingsService;
            _invalidate = invalidate;
        }

        public CarIcon GetImageIconOrPlaceholder(string? imageUri, int placeholderResourceId)
        {
            var placeholder = CreateResourceIcon(placeholderResourceId);
            var normalizedImageUri = NormalizeImageUri(imageUri);
            if (string.IsNullOrWhiteSpace(normalizedImageUri))
            {
                return placeholder;
            }

            lock (SharedLock)
            {
                if (SharedIconCache.TryGetValue(normalizedImageUri, out var cachedIcon))
                {
                    return cachedIcon;
                }
            }

            if (Uri.TryCreate(normalizedImageUri, UriKind.Absolute, out var remoteUri)
                && (string.Equals(remoteUri.Scheme, Uri.UriSchemeHttp, StringComparison.OrdinalIgnoreCase)
                    || string.Equals(remoteUri.Scheme, Uri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase)))
            {
                var shouldStartLoad = false;
                lock (SharedLock)
                {
                    if (SharedLoadingUris.Add(normalizedImageUri))
                    {
                        shouldStartLoad = true;
                    }
                }

                if (shouldStartLoad)
                {
                    _ = LoadRemoteIconAsync(normalizedImageUri, remoteUri);
                }

                return placeholder;
            }

            try
            {
                var uriIcon = IconCompat.CreateWithContentUri(normalizedImageUri);
                return new CarIcon.Builder(uriIcon).Build();
            }
            catch
            {
                return placeholder;
            }
        }

        private string? NormalizeImageUri(string? imageUri)
        {
            if (string.IsNullOrWhiteSpace(imageUri))
            {
                return null;
            }

            if (Uri.TryCreate(imageUri, UriKind.Absolute, out _))
            {
                return imageUri;
            }

            if (imageUri.StartsWith("/", StringComparison.Ordinal))
            {
                var baseUrl = _settingsService?.MusicAssistantUrl;
                if (!string.IsNullOrWhiteSpace(baseUrl))
                {
                    return string.Concat(baseUrl.TrimEnd('/'), imageUri);
                }
            }

            return imageUri;
        }

        private CarIcon CreateResourceIcon(int resourceId)
        {
            return new CarIcon.Builder(IconCompat.CreateWithResource(_carContext, resourceId)).Build();
        }

        private async Task LoadRemoteIconAsync(string imageUri, Uri remoteUri)
        {
            try
            {
                var bytes = await RemoteImageHttpClient.GetByteArrayAsync(remoteUri);
                if (bytes.Length == 0)
                {
                    return;
                }

                var bitmap = BitmapFactory.DecodeByteArray(bytes, 0, bytes.Length);
                if (bitmap == null)
                {
                    return;
                }

                var icon = new CarIcon.Builder(IconCompat.CreateWithBitmap(bitmap)).Build();
                lock (SharedLock)
                {
                    SharedIconCache[imageUri] = icon;
                }

                MainThread.BeginInvokeOnMainThread(_invalidate);
            }
            catch
            {
                // Keep placeholders when remote image loading fails.
            }
            finally
            {
                lock (SharedLock)
                {
                    SharedLoadingUris.Remove(imageUri);
                }
            }
        }
    }
}
