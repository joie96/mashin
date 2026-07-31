using FuzzySharp;
using mashin.Collections;
using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using System.Collections.Specialized;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class ListView : ContentView
{
    #region Bindable properties

    private const int PageSize = 9;

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(ListView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(ListView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(ListView));

    public static readonly BindableProperty ItemWidthProperty =
        BindableProperty.Create(nameof(ItemWidth), typeof(double), typeof(ListView), 320d);

    #endregion

    #region Fields

    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _itemsSourceCollection;
    private int _loadedItemCount = PageSize;
    private bool _hasMoreItems;

    #endregion

    #region Properties

    public IEnumerable<object>? ItemsSource
    {
        get => (IEnumerable<object>?)GetValue(ItemsSourceProperty);
        set => SetValue(ItemsSourceProperty, value);
    }

    public IMediaItemActions? MediaActions
    {
        get => (IMediaItemActions?)GetValue(MediaActionsProperty);
        set => SetValue(MediaActionsProperty, value);
    }

    public ICommand? PrimaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(PrimaryInfoTappedCommandProperty);
        set => SetValue(PrimaryInfoTappedCommandProperty, value);
    }

    public double ItemWidth
    {
        get => (double)GetValue(ItemWidthProperty);
        set => SetValue(ItemWidthProperty, value);
    }

    public bool HasMoreItems => _hasMoreItems;

    #endregion

    #region Construction

    public ListView()
    {
        InitializeComponent();
        ItemsCollectionView.ItemsSource = _visibleItems;
        RefreshVisibleItems();
    }

    #endregion

    #region Property Changed Handlers

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not ListView view)
        {
            return;
        }

        if (!ReferenceEquals(oldValue, newValue))
        {
            view._loadedItemCount = PageSize;
        }

        view.AttachItemsSourceCollection(newValue as IEnumerable<object>);
        view.RefreshVisibleItems();
    }

    #endregion

    #region ItemSource handling

    private void AttachItemsSourceCollection(IEnumerable<object>? source)
    {
        if (_itemsSourceCollection != null)
        {
            _itemsSourceCollection.CollectionChanged -= OnItemsSourceCollectionChanged;
            _itemsSourceCollection = null;
        }

        if (source is INotifyCollectionChanged collection)
        {
            _itemsSourceCollection = collection;
            _itemsSourceCollection.CollectionChanged += OnItemsSourceCollectionChanged;
        }
    }

    private void OnItemsSourceCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (!MainThread.IsMainThread)
        {
            MainThread.BeginInvokeOnMainThread(RefreshVisibleItems);
            return;
        }

        RefreshVisibleItems();
    }

    private void RefreshVisibleItems()
    {
        if (ItemsSource == null)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var sourceItems = ItemsSource.ToList();
        var totalCount = sourceItems.Count;
        if (totalCount == 0)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        var visibleCount = Math.Min(_loadedItemCount, totalCount);
        var nextItems = sourceItems.Take(visibleCount).ToList();

        _visibleItems.ReplaceRange(nextItems);
        _loadedItemCount = visibleCount;
        UpdateHasMoreItems(visibleCount < totalCount);
    }

    private void UpdateHasMoreItems(bool hasMoreItems)
    {
        if (_hasMoreItems == hasMoreItems)
        {
            return;
        }

        _hasMoreItems = hasMoreItems;
        OnPropertyChanged(nameof(HasMoreItems));
    }

    private void AppendVisibleItemsPage()
    {
        if (ItemsSource == null)
        {
            return;
        }

        var sourceItems = ItemsSource.ToList();
        var totalCount = sourceItems.Count;
        if (totalCount == 0)
        {
            _visibleItems.Clear();
            _loadedItemCount = 0;
            UpdateHasMoreItems(false);
            return;
        }

        if (_visibleItems.Count > totalCount)
        {
            RefreshVisibleItems();
            return;
        }

        var currentCount = _visibleItems.Count;
        var nextCount = Math.Min(currentCount + PageSize, totalCount);
        if (nextCount > currentCount)
        {
            _visibleItems.AddRange(sourceItems.Skip(currentCount).Take(nextCount - currentCount));
        }

        _loadedItemCount = nextCount;
        UpdateHasMoreItems(nextCount < totalCount);
    }

    #endregion

    #region UI event handlers

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not Border playButton || playButton.BindingContext is not MediaItem item)
        {
            return;
        }

        var playbackService = ResolvePlaybackService();
        if (playbackService == null)
        {
            return;
        }

        await playbackService.PlayMediaAsync(new List<MediaItem> { item });
    }

    private void OnLoadMoreTapped(object? sender, TappedEventArgs e)
    {
        if (ItemsSource == null)
        {
            return;
        }

        AppendVisibleItemsPage();
    }

    private void OnAccentBarLoaded(object? sender, EventArgs e)
    {
        if (sender is not Border accentBar)
        {
            return;
        }

        var text = accentBar.BindingContext switch
        {
            Playlist playlist => playlist.DisplayName,
            _ => null
        };

        accentBar.BackgroundColor = GetAccentColorFromText(text);
    }

    #endregion

    #region Helpers

    private static PlaybackService? ResolvePlaybackService()
    {
        var services = Application.Current?.Handler?.MauiContext?.Services;
        if (services == null)
        {
            return null;
        }

        return services.GetService<PlaybackService>();
    }

    private static Color GetAccentColorFromText(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return Color.FromArgb("#7CA8FF");
        }

        var normalized = text.Trim().ToLowerInvariant().Normalize(NormalizationForm.FormD);
        var normalizedBuilder = new StringBuilder(normalized.Length);
        foreach (var c in normalized)
        {
            var category = CharUnicodeInfo.GetUnicodeCategory(c);
            if (category != UnicodeCategory.NonSpacingMark)
            {
                normalizedBuilder.Append(char.IsLetterOrDigit(c) ? c : ' ');
            }
        }

        var normalizedText = Regex.Replace(normalizedBuilder.ToString(), "\\s+", " ").Trim();
        if (string.IsNullOrWhiteSpace(normalizedText))
        {
            return Color.FromArgb("#7CA8FF");
        }

        var tokens = normalizedText.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (tokens.Length == 0)
        {
            return Color.FromArgb("#7CA8FF");
        }

        var anchors = new (string Token, float Hue)[]
        {
            ("metal", 8f),
            ("rock", 20f),
            ("punk", 32f),
            ("electro", 56f),
            ("edm", 68f),
            ("house", 82f),
            ("techno", 98f),
            ("trance", 116f),
            ("ambient", 138f),
            ("chill", 152f),
            ("jazz", 172f),
            ("blues", 188f),
            ("funk", 206f),
            ("disco", 222f),
            ("pop", 244f),
            ("indie", 260f),
            ("folk", 276f),
            ("classical", 294f),
            ("orchestral", 306f),
            ("cinematic", 320f),
            ("soundtrack", 336f),
            ("hiphop", 350f)
        };

        var sumX = 0.0;
        var sumY = 0.0;
        var weightedSaturation = 0.0;
        var totalWeight = 0.0;

        var textFallbackHue = GetStableHueFromToken(normalizedText);
        foreach (var token in tokens)
        {
            var bestScore = 0;
            var bestHue = textFallbackHue;

            foreach (var anchor in anchors)
            {
                var score = Fuzz.TokenSetRatio(token, anchor.Token);
                if (score > bestScore)
                {
                    bestScore = score;
                    bestHue = anchor.Hue;
                }
            }

            // For weak fuzzy matches, spread the color by stable token hash instead of drifting toward red.
            var useAnchor = bestScore >= 58;
            var tokenHue = useAnchor ? bestHue : GetStableHueFromToken(token);
            var weight = useAnchor
                ? Math.Clamp(bestScore / 100.0, 0.35, 1.0)
                : 0.24;

            var tokenSaturation = useAnchor
                ? 0.52 + ((bestScore / 100.0) * 0.16)
                : 0.46;

            var radians = tokenHue * Math.PI / 180.0;
            sumX += Math.Cos(radians) * weight;
            sumY += Math.Sin(radians) * weight;
            weightedSaturation += tokenSaturation * weight;
            totalWeight += weight;
        }

        if (Math.Abs(sumX) < 0.0001 && Math.Abs(sumY) < 0.0001)
        {
            sumX = 1;
            sumY = 0;
        }

        var hue = (Math.Atan2(sumY, sumX) * 180.0 / Math.PI + 360.0) % 360.0;
        var saturation = (float)Math.Clamp(
            totalWeight > 0 ? weightedSaturation / totalWeight : 0.56,
            0.50,
            0.68);
        var lightness = 0.60f;

        return Color.FromHsla(hue / 360.0, saturation, lightness);
    }

    private static float GetStableHueFromToken(string token)
    {
        var hash = 2166136261u;
        foreach (var c in token)
        {
            hash ^= c;
            hash *= 16777619;
        }

        return hash % 360;
    }

    #endregion
}

public sealed class MobileListViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? PlaylistTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is ListViewSkeleton && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is Playlist && PlaylistTemplate != null)
        {
            return PlaylistTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("MobileListViewTemplateSelector requires PlaylistTemplate or SkeletonTemplate.");
    }
}

