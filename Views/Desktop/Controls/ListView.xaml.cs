using mashin.Collections;
using mashin.Models;
using mashin.Services;
using FuzzySharp;
using System.Collections.Specialized;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows.Input;

namespace mashin.Views.Desktop.Controls;

public partial class ListView : ContentView
{
    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(ListView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(ListView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(ListView));

    public static readonly BindableProperty MaxItemsProperty =
        BindableProperty.Create(nameof(MaxItems), typeof(int), typeof(ListView), 9, propertyChanged: OnMaxItemsChanged);

    #endregion

    #region Fields

    private readonly ObservableRangeCollection<object> _visibleItems = new();
    private INotifyCollectionChanged? _itemsSourceCollection;

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

    public int MaxItems
    {
        get => (int)GetValue(MaxItemsProperty);
        set => SetValue(MaxItemsProperty, value);
    }

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

        view.AttachItemsSourceCollection(newValue as IEnumerable<object>);
        view.RefreshVisibleItems();
    }

    private static void OnMaxItemsChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not ListView view)
        {
            return;
        }

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
        var source = ItemsSource;
        var maxItems = Math.Max(1, MaxItems);

        if (source == null)
        {
            _visibleItems.Clear();
            return;
        }

        var items = source.Take(maxItems).ToList();
        _visibleItems.ReplaceRange(items);
    }

    #endregion

    #region UI event handlers

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not Border playButton || playButton.BindingContext is not MediaItem item || MediaActions == null)
        {
            return;
        }

        await MediaActions.PlayMediaAsync(item);
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

    private static Color GetAccentColorFromText(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return Color.FromArgb("#5A8DFF");
        }

        // Normalize to improve fuzzy matching consistency across accents and punctuation.
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
            return Color.FromArgb("#5A8DFF");
        }

        var tokens = normalizedText.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (tokens.Length == 0)
        {
            return Color.FromArgb("#5A8DFF");
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
        foreach (var token in tokens)
        {
            var bestScore = 0;
            var bestHue = 0f;

            foreach (var anchor in anchors)
            {
                var score = Fuzz.TokenSetRatio(token, anchor.Token);
                if (score > bestScore)
                {
                    bestScore = score;
                    bestHue = anchor.Hue;
                }
            }

            var weight = Math.Clamp(bestScore / 100.0, 0.2, 1.0);
            var radians = bestHue * Math.PI / 180.0;
            sumX += Math.Cos(radians) * weight;
            sumY += Math.Sin(radians) * weight;
        }

        if (Math.Abs(sumX) < 0.0001 && Math.Abs(sumY) < 0.0001)
        {
            sumX = 1;
            sumY = 0;
        }

        var hue = (Math.Atan2(sumY, sumX) * 180.0 / Math.PI + 360.0) % 360.0;
        var saturation = 0.66f;
        var lightness = 0.52f;

        return Color.FromHsla(hue / 360.0, saturation, lightness);
    }

    #endregion
}

#region TemplateSelector

public sealed class ListViewTemplateSelector : DataTemplateSelector
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

        throw new InvalidOperationException("ListViewTemplateSelector requires PlaylistTemplate or SkeletonTemplate.");
    }
}

#endregion
