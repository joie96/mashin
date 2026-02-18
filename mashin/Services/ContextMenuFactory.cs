using mashin.Collections;
using mashin.Models;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;
using Microsoft.Extensions.Logging;
using System.Collections.ObjectModel;

namespace mashin.Services;

public interface IContextMenuFactory
{
    Task<ObservableRangeCollection<ContextMenuItem>> CreateAsync(
        MediaItem? item,
        IEnumerable<MediaItem>? selection = null,
        Playlist? playlistContext = null);
}

public sealed class ContextMenuFactory : IContextMenuFactory
{
    private readonly MusicAssistantService _musicAssistant;
    private readonly IUserDataService _userDataService;
    private readonly IMediaItemActions _mediaActions;
    private readonly ILogger<ContextMenuFactory> _logger;

    public ContextMenuFactory(
        MusicAssistantService musicAssistant,
        IUserDataService userDataService,
        IMediaItemActions mediaActions,
        ILogger<ContextMenuFactory> logger)
    {
        _musicAssistant = musicAssistant;
        _userDataService = userDataService;
        _mediaActions = mediaActions;
        _logger = logger;
    }

    public async Task<ObservableRangeCollection<ContextMenuItem>> CreateAsync(
        MediaItem? item, MediaItem? context = null)
    {
        if (item is Playlist playlist)
        {
            return BuildPlaylistMenu(playlist);
        }

        if (item is Track track)
        {
            return await BuildTrackMenuAsync(track, context);
        }

        return new ObservableRangeCollection<ContextMenuItem>();
    }

    private ObservableRangeCollection<ContextMenuItem> BuildPlaylistMenu(Playlist playlist)
    {
        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () => await _mediaActions.PlayMediaAsync(playlist))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () => await _mediaActions.PlayMediaNextAsync(playlist))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () => await _mediaActions.PlayMediaLastAsync(playlist))
            },
            new() { IsSeparator = true }
        };

        if (playlist.Favorite)
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Aus Favoriten entfernen",
                Icon = FluentFilledIcons.Heart12Filled,
                IconIsFilled = true,
                Command = new Command(async () => await _mediaActions.RemoveFromFavoritesAsync(playlist))
            });
        }
        else
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Zu Favoriten hinzufuegen",
                Icon = FluentIcons.Heart12,
                Command = new Command(async () => await _mediaActions.AddToFavoritesAsync(playlist))
            });
        }

        return menu;
    }

    private async Task<ObservableRangeCollection<ContextMenuItem>> BuildTrackMenuAsync(Track track, MediaItem? context)
    {
        var menu = new ObservableRangeCollection<ContextMenuItem>
        {
            new()
            {
                Text = "Abspielen",
                Icon = FluentIcons.Play12,
                Command = new Command(async () => await _mediaActions.PlayMediaAsync(selection))
            },
            new()
            {
                Text = "Als Naechstes spielen",
                Icon = FluentIcons.ArrowForward16,
                Command = new Command(async () => await _mediaActions.PlayMediaNextAsync(selection))
            },
            new()
            {
                Text = "Als Letztes spielen",
                Icon = FluentIcons.ArrowNext12,
                Command = new Command(async () => await _mediaActions.PlayMediaLastAsync(selection))
            },
            new() { IsSeparator = true },
            new()
            {
                Text = "Zu Wiedergabeliste hinzufuegen",
                Icon = FluentIcons.Add12,
                SubItems = await GetPlaylistSubItemsAsync(selection)
            }
        };

        if (playlistContext != null)
        {
            menu.Add(new ContextMenuItem
            {
                Text = "Aus Wiedergabeliste entfernen",
                Icon = FluentIcons.Subtract12,
                Command = new Command(async () => await _mediaActions.RemoveFromPlaylistAsync(selection, playlistContext))
            });
        }

        menu.Add(new ContextMenuItem { IsSeparator = true });

        menu.Add(new ContextMenuItem
        {
            Text = "Zu Favoriten hinzufuegen",
            Icon = FluentIcons.Heart12,
            Command = new Command(async () => await _mediaActions.AddToFavoritesAsync(selection))
        });

        menu.Add(new ContextMenuItem
        {
            Text = "Aus Favoriten entfernen",
            Icon = FluentFilledIcons.Heart12Filled,
            IconIsFilled = true,
            Command = new Command(async () => await _mediaActions.RemoveFromFavoritesAsync(selection))
        });

        return menu;
    }


    #region Helper Methods
    private async Task<ObservableCollection<ContextMenuItem>> GetPlaylistSubItemsAsync(
        IReadOnlyCollection<MediaItem> selection)
    {
        var items = new ObservableCollection<ContextMenuItem>();

        try
        {
            var playlists = await _musicAssistant.GetLibraryPlaylistsAsync(orderBy: "sort_name");
            ApplyPlaylistDisplayNames(playlists);

            foreach (var playlist in playlists)
            {
                if (playlist.Name.StartsWith("~", StringComparison.Ordinal))
                {
                    continue;
                }

                items.Add(new ContextMenuItem
                {
                    Text = playlist.DisplayName,
                    Icon = FluentIcons.Add12,
                    Command = new Command(async () =>
                        await _mediaActions.AddToPlaylistAsync(selection, playlist))
                });
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to build playlist subitems for context menu");
        }

        return items;
    }

    private void ApplyPlaylistDisplayNames(IEnumerable<Playlist> playlists)
    {
        var prefix = GetUserPlaylistPrefix();

        foreach (var playlist in playlists)
        {
            playlist.DisplayName = playlist.Name;

            if (!string.IsNullOrWhiteSpace(prefix)
                && !string.IsNullOrWhiteSpace(playlist.Name)
                && playlist.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            {
                playlist.DisplayName = playlist.Name[prefix.Length..];
            }
        }
    }

    private string? GetUserPlaylistPrefix()
    {
        var username = _userDataService.CurrentUser?.Username;
        if (string.IsNullOrWhiteSpace(username))
        {
            return null;
        }

        return string.Concat(username, "--");
    }

    #endregion
}
