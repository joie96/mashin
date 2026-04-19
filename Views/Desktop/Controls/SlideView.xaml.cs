using mashin.Models;
using mashin.Services;
using MauiColor = Microsoft.Maui.Graphics.Color;
using MauiPoint = Microsoft.Maui.Graphics.Point;
using ImageSharpImage = SixLabors.ImageSharp.Image;
using SixLabors.ImageSharp.PixelFormats;
using System.Collections.Concurrent;
using System.Collections.Specialized;
using System.Windows.Input;

namespace mashin.Views.Desktop.Controls;

public partial class SlideView : ContentView
{
    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(SlideView), propertyChanged: OnItemsSourceChanged);

    public static readonly BindableProperty MediaActionsProperty =
        BindableProperty.Create(nameof(MediaActions), typeof(IMediaItemActions), typeof(SlideView));

    public static readonly BindableProperty PrimaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(PrimaryInfoTappedCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty CoverSizeProperty =
        BindableProperty.Create(nameof(CoverSize), typeof(double), typeof(SlideView), 250d);

    public static readonly BindableProperty ItemCornerRadiusProperty =
        BindableProperty.Create(nameof(ItemCornerRadius), typeof(float), typeof(SlideView), 8f);

    public static readonly BindableProperty CurrentItemProperty =
        BindableProperty.Create(nameof(CurrentItem), typeof(object), typeof(SlideView), null, BindingMode.TwoWay, propertyChanged: OnCurrentItemChanged);

    public static readonly BindableProperty CurrentIndexProperty =
        BindableProperty.Create(nameof(CurrentIndex), typeof(int), typeof(SlideView), 0, BindingMode.TwoWay, propertyChanged: OnCurrentIndexChanged);

    public static readonly BindableProperty CanGoPrevProperty =
        BindableProperty.Create(nameof(CanGoPrev), typeof(bool), typeof(SlideView), false);

    public static readonly BindableProperty CanGoNextProperty =
        BindableProperty.Create(nameof(CanGoNext), typeof(bool), typeof(SlideView), false);

    public static readonly BindableProperty CurrentPageProperty =
        BindableProperty.Create(nameof(CurrentPage), typeof(int), typeof(SlideView), 1, BindingMode.TwoWay, propertyChanged: OnCurrentPageChanged);

    public static readonly BindableProperty TotalPagesProperty =
        BindableProperty.Create(nameof(TotalPages), typeof(int), typeof(SlideView), 1, BindingMode.TwoWay);

    #endregion

    #region Fields

    private static readonly HttpClient PaletteHttpClient = new() { Timeout = TimeSpan.FromSeconds(8) };
    private static readonly MauiColor DefaultSlideCardColor = MauiColor.FromArgb("#293548");

    private readonly ConcurrentDictionary<string, MauiColor> _dominantColorCache = new(StringComparer.OrdinalIgnoreCase);
    private readonly List<object> _allItems = new();

    private INotifyCollectionChanged? _itemsSourceCollection;
    private bool _isSynchronizingExternalState;

    private Grid? SlideContentHostElement => this.FindByName<Grid>("SlideContentHost");
    private Border? SlideCardBorderElement => this.FindByName<Border>("SlideCardBorder");

    #endregion

    #region Public properties

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

    public ICommand? ShowContextMenuAtAnchorCommand
    {
        get => (ICommand?)GetValue(ShowContextMenuAtAnchorCommandProperty);
        set => SetValue(ShowContextMenuAtAnchorCommandProperty, value);
    }

    public ICommand? ShowContextMenuAtPositionCommand
    {
        get => (ICommand?)GetValue(ShowContextMenuAtPositionCommandProperty);
        set => SetValue(ShowContextMenuAtPositionCommandProperty, value);
    }

    public double CoverSize
    {
        get => (double)GetValue(CoverSizeProperty);
        set => SetValue(CoverSizeProperty, value);
    }

    public float ItemCornerRadius
    {
        get => (float)GetValue(ItemCornerRadiusProperty);
        set => SetValue(ItemCornerRadiusProperty, value);
    }

    public object? CurrentItem
    {
        get => GetValue(CurrentItemProperty);
        set => SetValue(CurrentItemProperty, value);
    }

    public int CurrentIndex
    {
        get => (int)GetValue(CurrentIndexProperty);
        set => SetValue(CurrentIndexProperty, value);
    }

    public bool CanGoPrev
    {
        get => (bool)GetValue(CanGoPrevProperty);
        private set => SetValue(CanGoPrevProperty, value);
    }

    public bool CanGoNext
    {
        get => (bool)GetValue(CanGoNextProperty);
        private set => SetValue(CanGoNextProperty, value);
    }

    public int CurrentPage
    {
        get => (int)GetValue(CurrentPageProperty);
        set => SetValue(CurrentPageProperty, value);
    }

    public int TotalPages
    {
        get => (int)GetValue(TotalPagesProperty);
        set => SetValue(TotalPagesProperty, value);
    }

    public ICommand PrevPageCommand { get; }
    public ICommand NextPageCommand { get; }

    #endregion

    #region Construction

    public SlideView()
    {
        InitializeComponent();

        PrevPageCommand = new Command(() => GoToPreviousIndex(), () => CanGoPrev);
        NextPageCommand = new Command(() => GoToNextIndex(), () => CanGoNext);

        SetDefaultBackground();
        UpdateItemStateFromSource();
    }

    #endregion

    #region Property Changed Handlers

    private static void OnItemsSourceChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not SlideView view)
        {
            return;
        }

        view.ApplyItemsSource(newValue as IEnumerable<object>);
    }

    private static void OnCurrentItemChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not SlideView view)
        {
            return;
        }

        view.SyncIndexFromCurrentItem();
        view.OnCurrentItemChangedAsync(newValue).SafeFireAndForget();
    }

    private static void OnCurrentIndexChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not SlideView view)
        {
            return;
        }

        view.SyncCurrentItemFromIndex();
    }

    private static void OnCurrentPageChanged(BindableObject bindable, object oldValue, object newValue)
    {
        if (bindable is not SlideView view)
        {
            return;
        }

        if (view._isSynchronizingExternalState)
        {
            return;
        }

        var requestedPage = (int)newValue;
        if (requestedPage <= 0)
        {
            requestedPage = 1;
        }

        view.CurrentIndex = requestedPage - 1;
    }

    #endregion

    #region ItemSource handling

    private void ApplyItemsSource(IEnumerable<object>? items)
    {
        DetachItemsSourceCollection();

        _allItems.Clear();
        if (items != null)
        {
            _allItems.AddRange(items);
        }

        if (items is INotifyCollectionChanged notifyCollection)
        {
            _itemsSourceCollection = notifyCollection;
            _itemsSourceCollection.CollectionChanged += OnItemsSourceCollectionChanged;
        }

        UpdateItemStateFromSource();
    }

    private void DetachItemsSourceCollection()
    {
        if (_itemsSourceCollection == null)
        {
            return;
        }

        _itemsSourceCollection.CollectionChanged -= OnItemsSourceCollectionChanged;
        _itemsSourceCollection = null;
    }

    private void OnItemsSourceCollectionChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (!MainThread.IsMainThread)
        {
            MainThread.BeginInvokeOnMainThread(() => OnItemsSourceCollectionChanged(sender, e));
            return;
        }

        _allItems.Clear();
        if (ItemsSource != null)
        {
            _allItems.AddRange(ItemsSource);
        }

        UpdateItemStateFromSource();
    }

    #endregion

    #region Index synchronization

    private void UpdateItemStateFromSource()
    {
        _isSynchronizingExternalState = true;
        try
        {
            TotalPages = Math.Max(1, _allItems.Count);

            if (_allItems.Count == 0)
            {
                CurrentIndex = 0;
                CurrentPage = 1;
                CurrentItem = null;
                return;
            }

            var clampedIndex = Math.Clamp(CurrentIndex, 0, _allItems.Count - 1);
            if (clampedIndex != CurrentIndex)
            {
                CurrentIndex = clampedIndex;
            }

            CurrentPage = clampedIndex + 1;
            CurrentItem = _allItems[clampedIndex];
        }
        finally
        {
            _isSynchronizingExternalState = false;
        }

        UpdateIndexNavigationState();
    }

    private void SyncCurrentItemFromIndex()
    {
        if (_isSynchronizingExternalState)
        {
            return;
        }

        if (_allItems.Count == 0)
        {
            _isSynchronizingExternalState = true;
            try
            {
                CurrentPage = 1;
                TotalPages = 1;
                CurrentItem = null;
            }
            finally
            {
                _isSynchronizingExternalState = false;
            }

            return;
        }

        var clampedIndex = Math.Clamp(CurrentIndex, 0, _allItems.Count - 1);
        var nextItem = _allItems[clampedIndex];

        _isSynchronizingExternalState = true;
        try
        {
            if (CurrentIndex != clampedIndex)
            {
                CurrentIndex = clampedIndex;
            }

            TotalPages = Math.Max(1, _allItems.Count);
            CurrentPage = clampedIndex + 1;
            CurrentItem = nextItem;
        }
        finally
        {
            _isSynchronizingExternalState = false;
        }

        UpdateIndexNavigationState();
    }

    private void SyncIndexFromCurrentItem()
    {
        if (_isSynchronizingExternalState)
        {
            return;
        }

        if (_allItems.Count == 0)
        {
            return;
        }

        var index = _allItems.FindIndex(item => ReferenceEquals(item, CurrentItem));
        if (index < 0)
        {
            return;
        }

        _isSynchronizingExternalState = true;
        try
        {
            CurrentIndex = index;
            CurrentPage = index + 1;
            TotalPages = Math.Max(1, _allItems.Count);
        }
        finally
        {
            _isSynchronizingExternalState = false;
        }

        UpdateIndexNavigationState();
    }

    #endregion

    #region Index navigation commands

    private void GoToPreviousIndex()
    {
        if (!CanGoPrev)
        {
            return;
        }

        CurrentIndex = Math.Max(0, CurrentIndex - 1);
    }

    private void GoToNextIndex()
    {
        if (!CanGoNext)
        {
            return;
        }

        CurrentIndex = Math.Min(Math.Max(0, _allItems.Count - 1), CurrentIndex + 1);
    }

    private void UpdateIndexNavigationState()
    {
        if (_allItems.Count <= 0)
        {
            CanGoPrev = false;
            CanGoNext = false;
        }
        else
        {
            var clampedIndex = Math.Clamp(CurrentIndex, 0, _allItems.Count - 1);
            CanGoPrev = clampedIndex > 0;
            CanGoNext = clampedIndex < _allItems.Count - 1;
        }

        if (PrevPageCommand is Command prev)
        {
            prev.ChangeCanExecute();
        }

        if (NextPageCommand is Command next)
        {
            next.ChangeCanExecute();
        }
    }

    #endregion

    #region UI event handlers

    private async Task OnCurrentItemChangedAsync(object? item)
    {
        var slideContentHost = SlideContentHostElement;
        if (slideContentHost != null)
        {
            slideContentHost.BindingContext = item;
        }

        await ApplyTrackCardPaletteAsync(item as Track);
    }

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (CurrentItem is not MediaItem item || MediaActions == null)
        {
            return;
        }

        await MediaActions.PlayMediaAsync(item);
    }

    private async void OnFavoriteIconTapped(object? sender, TappedEventArgs e)
    {
        if (CurrentItem is not MediaItem item)
        {
            return;
        }

        if (MediaActions == null)
        {
            item.Favorite = !item.Favorite;
            return;
        }

        if (item.Favorite)
        {
            await MediaActions.RemoveFromFavoritesAsync(item);
        }
        else
        {
            await MediaActions.AddToFavoritesAsync(item);
        }
    }

    private void OnMoreIconTapped(object? sender, TappedEventArgs e)
    {
        if (sender is not Label anchor)
        {
            return;
        }

        ShowContextMenuAtAnchorCommand?.Execute(anchor);
    }

    private void OnCoverSecondaryPointerPressed(object? sender, PointerEventArgs e)
    {
        var position = e.GetPosition(null);
        ShowContextMenuAtPositionCommand?.Execute(position);
    }

    private void OnPrimaryInfoTapped(object? sender, TappedEventArgs e)
    {
        if (CurrentItem == null)
        {
            return;
        }

        if (PrimaryInfoTappedCommand?.CanExecute(CurrentItem) == true)
        {
            PrimaryInfoTappedCommand.Execute(CurrentItem);
        }
    }

    #endregion

    #region Card palette

    private async Task ApplyTrackCardPaletteAsync(Track? track)
    {
        var slideCardBorder = SlideCardBorderElement;
        if (slideCardBorder == null)
        {
            return;
        }

        if (track == null)
        {
            SetDefaultBackground();
            return;
        }

        var baseColor = await GetDominantColorAsync(track.ImageUrl);
        var topColor = Lighten(baseColor, 0.16f);
        var centerColor = Saturate(baseColor, 1.08f);
        var bottomColor = Darken(baseColor, 0.28f);

        await MainThread.InvokeOnMainThreadAsync(() =>
        {
            slideCardBorder.Background = new LinearGradientBrush(
                new GradientStopCollection
                {
                    new GradientStop(topColor, 0f),
                    new GradientStop(centerColor, 0.56f),
                    new GradientStop(bottomColor, 1f)
                },
                new MauiPoint(0, 0),
                new MauiPoint(1, 1));

            slideCardBorder.Shadow = new Shadow
            {
                Brush = new SolidColorBrush(Darken(centerColor, 0.45f).WithAlpha(0.72f)),
                Offset = new MauiPoint(0, 18),
                Radius = 28,
                Opacity = 0.95f
            };
        });
    }

    private void SetDefaultBackground()
    {
        var slideCardBorder = SlideCardBorderElement;
        if (slideCardBorder == null)
        {
            return;
        }

        slideCardBorder.Background = new LinearGradientBrush(
            new GradientStopCollection
            {
                new GradientStop(MauiColor.FromArgb("#6F5330"), 0f),
                new GradientStop(MauiColor.FromArgb("#A7602E"), 0.6f),
                new GradientStop(MauiColor.FromArgb("#8D3527"), 1f)
            },
            new MauiPoint(0, 0),
            new MauiPoint(1, 1));

        slideCardBorder.Shadow = new Shadow { Opacity = 0 };
    }

    private async Task<MauiColor> GetDominantColorAsync(string? imageUrl)
    {
        if (string.IsNullOrWhiteSpace(imageUrl))
        {
            return DefaultSlideCardColor;
        }

        if (_dominantColorCache.TryGetValue(imageUrl, out var cachedColor))
        {
            return cachedColor;
        }

        var computed = await ComputeDominantColorAsync(imageUrl);
        _dominantColorCache[imageUrl] = computed;
        return computed;
    }

    #endregion

    #region Color analysis

    private static async Task<MauiColor> ComputeDominantColorAsync(string imageUrl)
    {
        try
        {
            if (!Uri.TryCreate(imageUrl, UriKind.Absolute, out _))
            {
                return DefaultSlideCardColor;
            }

            var bytes = await PaletteHttpClient.GetByteArrayAsync(imageUrl);
            if (bytes.Length == 0)
            {
                return DefaultSlideCardColor;
            }

            await using var ms = new MemoryStream(bytes);
            using var image = await ImageSharpImage.LoadAsync<Rgba32>(ms);

            var width = image.Width;
            var height = image.Height;
            if (width == 0 || height == 0)
            {
                return DefaultSlideCardColor;
            }

            var step = Math.Max(1, Math.Max(width, height) / 42);
            double weightedR = 0;
            double weightedG = 0;
            double weightedB = 0;
            double totalWeight = 0;

            for (var y = 0; y < height; y += step)
            {
                for (var x = 0; x < width; x += step)
                {
                    var px = image[x, y];
                    if (px.A < 20)
                    {
                        continue;
                    }

                    var r = px.R;
                    var g = px.G;
                    var b = px.B;

                    var max = Math.Max(r, Math.Max(g, b));
                    var min = Math.Min(r, Math.Min(g, b));
                    var saturation = max == 0 ? 0.0 : (max - min) / (double)max;
                    var luminance = (0.2126 * r) + (0.7152 * g) + (0.0722 * b);

                    var weight = (0.35 + (0.65 * saturation)) * (px.A / 255.0);
                    if (luminance < 16 || luminance > 242)
                    {
                        weight *= 0.35;
                    }

                    weightedR += r * weight;
                    weightedG += g * weight;
                    weightedB += b * weight;
                    totalWeight += weight;
                }
            }

            if (totalWeight <= 0.001)
            {
                return DefaultSlideCardColor;
            }

            var baseColor = MauiColor.FromRgba(
                (float)(weightedR / totalWeight) / 255f,
                (float)(weightedG / totalWeight) / 255f,
                (float)(weightedB / totalWeight) / 255f,
                1f);

            return Saturate(Lighten(baseColor, 0.03f), 1.1f);
        }
        catch
        {
            return DefaultSlideCardColor;
        }
    }

    private static MauiColor Lighten(MauiColor color, float amount)
        => Blend(color, Colors.White, amount);

    private static MauiColor Darken(MauiColor color, float amount)
        => Blend(color, Colors.Black, amount);

    private static MauiColor Saturate(MauiColor color, float factor)
    {
        var avg = (color.Red + color.Green + color.Blue) / 3f;
        var r = Clamp01(avg + ((color.Red - avg) * factor));
        var g = Clamp01(avg + ((color.Green - avg) * factor));
        var b = Clamp01(avg + ((color.Blue - avg) * factor));
        return MauiColor.FromRgba(r, g, b, color.Alpha);
    }

    private static MauiColor Blend(MauiColor from, MauiColor to, float amount)
    {
        var t = Clamp01(amount);
        return MauiColor.FromRgba(
            from.Red + ((to.Red - from.Red) * t),
            from.Green + ((to.Green - from.Green) * t),
            from.Blue + ((to.Blue - from.Blue) * t),
            from.Alpha + ((to.Alpha - from.Alpha) * t));
    }

    private static float Clamp01(float value)
        => Math.Max(0f, Math.Min(1f, value));

    #endregion

    #region Lifecycle

    protected override void OnHandlerChanging(HandlerChangingEventArgs args)
    {
        base.OnHandlerChanging(args);

        if (args.NewHandler == null)
        {
            DetachItemsSourceCollection();
        }
    }

    #endregion
}

internal static class TaskExtensions
{
    public static async void SafeFireAndForget(this Task task)
    {
        try
        {
            await task;
        }
        catch
        {
            // Ignore background task failures for visual updates.
        }
    }
}
