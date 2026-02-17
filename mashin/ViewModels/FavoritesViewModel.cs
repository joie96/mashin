using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.Logging;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using System.Linq;

namespace mashin.ViewModels
{
    public sealed class FavoritesViewModel : INotifyPropertyChanged, IDisposable
    {
        private readonly MusicAssistantService _musicAssistant;
        private readonly ILogger<FavoritesViewModel> _logger;

        private bool _isLoadingPlaylists;
        private bool _disposed;

        public FavoritesViewModel(
            MusicAssistantService musicAssistant,
            ILogger<FavoritesViewModel> logger)
        {
            _musicAssistant = musicAssistant;
            _logger = logger;

            Playlists = new ObservableCollection<Playlist>();
        }

        public ObservableCollection<Playlist> Playlists { get; }

        public IEnumerable<Playlist> PlaylistsLimited => Playlists.Take(20);

        public bool IsLoadingPlaylists
        {
            get => _isLoadingPlaylists;
            private set => SetProperty(ref _isLoadingPlaylists, value);
        }

        public async Task InitializeAsync()
        {
            await LoadPlaylistsAsync();
        }

        private async Task LoadPlaylistsAsync()
        {
            if (IsLoadingPlaylists)
                return;

            try
            {
                IsLoadingPlaylists = true;

                var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(limit: 50, orderBy: "sort_name");
                Playlists.Clear();
                foreach (var p in playlists)
                    Playlists.Add(p);


                _logger.LogInformation("Loaded {Count} playlists (Favorites)", Playlists.Count);
            }
            catch (System.Exception ex)
            {
                _logger.LogError(ex, "Failed to load playlists (Favorites)");
            }
            finally
            {
                IsLoadingPlaylists = false;
            }
        }

        public event PropertyChangedEventHandler? PropertyChanged;

        private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
            => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

        private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
        {
            if (EqualityComparer<T>.Default.Equals(field, value))
                return false;

            field = value;
            OnPropertyChanged(propertyName);
            return true;
        }

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;

            Playlists.Clear();
            PropertyChanged = null;
        }
    }
}