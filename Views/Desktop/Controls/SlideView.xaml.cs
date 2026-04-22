using mashin.Models;
using mashin.Services;
using mashin.Collections;
using ImageSharpImage = SixLabors.ImageSharp.Image;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Formats.Png;
using SixLabors.ImageSharp.Processing;
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

    public static readonly BindableProperty SecondaryInfoTappedCommandProperty =
        BindableProperty.Create(nameof(SecondaryInfoTappedCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty ShowContextMenuAtAnchorCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtAnchorCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty ShowContextMenuAtPositionCommandProperty =
        BindableProperty.Create(nameof(ShowContextMenuAtPositionCommand), typeof(ICommand), typeof(SlideView));

    public static readonly BindableProperty CoverSizeProperty =
        BindableProperty.Create(nameof(CoverSize), typeof(double), typeof(SlideView), 200d);

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
    private readonly ConcurrentDictionary<string, byte[]> _blurredCoverCache = new(StringComparer.OrdinalIgnoreCase);

    private readonly List<object> _allItems = new();
    private readonly ObservableRangeCollection<object> _visibleItemsPrimary = new();
    private readonly ObservableRangeCollection<object> _visibleItemsSecondary = new();

    private INotifyCollectionChanged? _itemsSourceCollection;
    private CancellationTokenSource? _pagingAnimationCts;
    private int? _pendingTargetIndex;
    private int? _inFlightTargetIndex;
    private bool _isSynchronizingExternalState;
    private bool _isAnimating;
    private bool _isPrimaryHostActive = true;

    private Border? SlideCardBorderPrimaryElement => this.FindByName<Border>("SlideCardBorderPrimary");
    private Border? SlideCardBorderSecondaryElement => this.FindByName<Border>("SlideCardBorderSecondary");
    private Image? SlideCardBackgroundImagePrimaryElement => this.FindByName<Image>("SlideCardBackgroundImagePrimary");
    private Image? SlideCardBackgroundImageSecondaryElement => this.FindByName<Image>("SlideCardBackgroundImageSecondary");
    private Grid? SlideItemHostPrimaryElement => this.FindByName<Grid>("SlideItemHostPrimary");
    private Grid? SlideItemHostSecondaryElement => this.FindByName<Grid>("SlideItemHostSecondary");

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

    public ICommand? SecondaryInfoTappedCommand
    {
        get => (ICommand?)GetValue(SecondaryInfoTappedCommandProperty);
        set => SetValue(SecondaryInfoTappedCommandProperty, value);
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

    private ObservableRangeCollection<object> ActiveVisibleItems => _isPrimaryHostActive ? _visibleItemsPrimary : _visibleItemsSecondary;
    private ObservableRangeCollection<object> InactiveVisibleItems => _isPrimaryHostActive ? _visibleItemsSecondary : _visibleItemsPrimary;
    private Border? ActiveCardBorder => _isPrimaryHostActive ? SlideCardBorderPrimaryElement : SlideCardBorderSecondaryElement;
    private Border? InactiveCardBorder => _isPrimaryHostActive ? SlideCardBorderSecondaryElement : SlideCardBorderPrimaryElement;
    private Image? ActiveBackgroundImage => _isPrimaryHostActive ? SlideCardBackgroundImagePrimaryElement : SlideCardBackgroundImageSecondaryElement;
    private Image? InactiveBackgroundImage => _isPrimaryHostActive ? SlideCardBackgroundImageSecondaryElement : SlideCardBackgroundImagePrimaryElement;

    public SlideView()
    {
        InitializeComponent();

        PrevPageCommand = new Command(async () => await GoToPreviousIndexAsync(), () => CanGoPrev);
        NextPageCommand = new Command(async () => await GoToNextIndexAsync(), () => CanGoNext);

        if (SlideItemHostPrimaryElement != null)
        {
            BindableLayout.SetItemsSource(SlideItemHostPrimaryElement, _visibleItemsPrimary);
        }

        if (SlideItemHostSecondaryElement != null)
        {
            BindableLayout.SetItemsSource(SlideItemHostSecondaryElement, _visibleItemsSecondary);
        }

        EnsureSingleActiveHostVisible();
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
        view.HandleCurrentItemChanged(newValue);
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

    private async Task GoToPreviousIndexAsync()
    {
        var baseIndex = GetNavigationCursorIndex();
        if (baseIndex <= 0)
        {
            return;
        }

        await SetIndexAsync(baseIndex - 1);
    }

    private async Task GoToNextIndexAsync()
    {
        var lastIndex = Math.Max(0, _allItems.Count - 1);
        var baseIndex = GetNavigationCursorIndex();
        if (baseIndex >= lastIndex)
        {
            return;
        }

        await SetIndexAsync(baseIndex + 1);
    }

    private async Task SetIndexAsync(int targetIndex)
    {
        if (_allItems.Count == 0)
        {
            return;
        }

        var clampedIndex = Math.Clamp(targetIndex, 0, _allItems.Count - 1);

        if (_isAnimating)
        {
            _pendingTargetIndex = clampedIndex;
            _pagingAnimationCts?.Cancel();
            return;
        }

        _pendingTargetIndex = clampedIndex;
        _isAnimating = true;
        try
        {
            while (_pendingTargetIndex.HasValue)
            {
                var nextIndex = Math.Clamp(_pendingTargetIndex.Value, 0, _allItems.Count - 1);
                _pendingTargetIndex = null;

                if (nextIndex == CurrentIndex)
                {
                    continue;
                }

                _pagingAnimationCts?.Dispose();
                _pagingAnimationCts = new CancellationTokenSource();
                _inFlightTargetIndex = nextIndex;

                try
                {
                    await AnimateToIndexAsync(nextIndex, _pagingAnimationCts.Token);
                }
                catch (OperationCanceledException)
                {
                    // A newer click canceled this transition; loop and apply latest pending target.
                }
                finally
                {
                    _inFlightTargetIndex = null;
                }
            }
        }
        finally
        {
            _pagingAnimationCts?.Dispose();
            _pagingAnimationCts = null;
            _pendingTargetIndex = null;
            _inFlightTargetIndex = null;
            _isAnimating = false;
        }
    }

    private int GetNavigationCursorIndex()
        => _pendingTargetIndex ?? _inFlightTargetIndex ?? CurrentIndex;

    private async Task AnimateToIndexAsync(int targetIndex, CancellationToken cancellationToken)
    {
        var direction = targetIndex > CurrentIndex ? -1 : 1;
        var slideOffset = 20;
        var outX = slideOffset * direction;
        var inX = -outX;
        const uint animationDurationMs = 250;

        var targetItem = _allItems[targetIndex];
        var activeCard = ActiveCardBorder;
        var inactiveCard = InactiveCardBorder;

        if (targetItem == null || activeCard == null || inactiveCard == null)
        {
            return;
        }

        InactiveVisibleItems.Clear();
        InactiveVisibleItems.Add(targetItem);

        await ApplySlideCardBackgroundToCardAsync(InactiveBackgroundImage, targetItem as MediaItem);

        inactiveCard.IsVisible = true;
        inactiveCard.InputTransparent = true;
        inactiveCard.Opacity = 0;
        inactiveCard.TranslationX = inX;

        try
        {
            await Task.WhenAll(
                activeCard.TranslateToAsync(outX, 0, animationDurationMs, Easing.CubicIn),
                activeCard.FadeToAsync(0, animationDurationMs, Easing.CubicIn)).WaitAsync(cancellationToken);

            await Task.WhenAll(
                inactiveCard.TranslateToAsync(0, 0, animationDurationMs, Easing.CubicOut),
                inactiveCard.FadeToAsync(1, animationDurationMs, Easing.CubicOut)).WaitAsync(cancellationToken);

            // Animation completed successfully
            _isPrimaryHostActive = !_isPrimaryHostActive;
            CurrentIndex = targetIndex;
            EnsureSingleActiveHostVisible();
            InactiveVisibleItems.Clear();
        }
        catch (OperationCanceledException)
        {
            // Animation was canceled - immediately commit the in-flight page and prepare for next animation
            activeCard.CancelAnimations();
            inactiveCard.CancelAnimations();

            // Swap hosts immediately: inactive becomes active
            _isPrimaryHostActive = !_isPrimaryHostActive;
            CurrentIndex = targetIndex;
            EnsureSingleActiveHostVisible();
            InactiveVisibleItems.Clear();

            throw;
        }
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

    private async void HandleCurrentItemChanged(object? item)
    {
        SyncVisibleItem(item);
        await ApplySlideCardBackgroundToCardAsync(ActiveBackgroundImage, item as MediaItem);
    }

    private void SyncVisibleItem(object? item)
    {
        if (item == null)
        {
            ActiveVisibleItems.Clear();
            return;
        }

        if (ActiveVisibleItems.Count == 1 && ReferenceEquals(ActiveVisibleItems[0], item))
        {
            return;
        }

        ActiveVisibleItems.Clear();
        ActiveVisibleItems.Add(item);
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

    #endregion

    #region Card palette

    private async Task ApplySlideCardBackgroundToCardAsync(Image? backgroundImage, MediaItem? mediaItem)
    {
        if (backgroundImage == null)
        {
            return;
        }

        if (mediaItem == null)
        {
            backgroundImage.Source = null;
            return;
        }

        var blurredCoverSource = await GetBlurredCoverSourceAsync(mediaItem.ImageUrl);

        await MainThread.InvokeOnMainThreadAsync(() =>
        {
            backgroundImage.Source = blurredCoverSource;
        });
    }

    private async Task<ImageSource?> GetBlurredCoverSourceAsync(string? imageUrl)
    {
        if (string.IsNullOrWhiteSpace(imageUrl))
        {
            return null;
        }

        if (_blurredCoverCache.TryGetValue(imageUrl, out var cachedBytes))
        {
            return ImageSource.FromStream(() => new MemoryStream(cachedBytes));
        }

        try
        {
            if (!Uri.TryCreate(imageUrl, UriKind.Absolute, out _))
            {
                return null;
            }

            var bytes = await PaletteHttpClient.GetByteArrayAsync(imageUrl);
            if (bytes.Length == 0)
            {
                return null;
            }

            await using var sourceStream = new MemoryStream(bytes);
            using var image = await ImageSharpImage.LoadAsync<Rgba32>(sourceStream);

            image.Mutate(ctx =>
            {
                ctx.GaussianBlur(24f);
            });

            await using var outputStream = new MemoryStream();
            await image.SaveAsync(outputStream, new PngEncoder());

            var blurredBytes = outputStream.ToArray();
            _blurredCoverCache[imageUrl] = blurredBytes;

            return ImageSource.FromStream(() => new MemoryStream(blurredBytes));
        }
        catch
        {
            return null;
        }
    }

    #endregion

    #region Host management

    private void EnsureSingleActiveHostVisible()
    {
        var activeCard = ActiveCardBorder;
        var inactiveCard = InactiveCardBorder;

        if (activeCard != null)
        {
            activeCard.IsVisible = true;
            activeCard.InputTransparent = false;
            activeCard.Opacity = 1;
            activeCard.TranslationX = 0;
        }

        if (inactiveCard != null)
        {
            inactiveCard.IsVisible = false;
            inactiveCard.InputTransparent = true;
            inactiveCard.Opacity = 0;
            inactiveCard.TranslationX = 0;
        }
    }

    #endregion

    #region Lifecycle

    protected override void OnHandlerChanging(HandlerChangingEventArgs args)
    {
        base.OnHandlerChanging(args);

        if (args.NewHandler == null)
        {
            _pagingAnimationCts?.Cancel();
            _pagingAnimationCts?.Dispose();
            _pagingAnimationCts = null;
            DetachItemsSourceCollection();
        }
    }

    #endregion
}

public sealed class SlideViewTemplateSelector : DataTemplateSelector
{
    public DataTemplate? TrackTemplate { get; set; }
    public DataTemplate? SkeletonTemplate { get; set; }

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container)
    {
        if (item is SkeletonItem && SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        if (item is Track && TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (TrackTemplate != null)
        {
            return TrackTemplate;
        }

        if (SkeletonTemplate != null)
        {
            return SkeletonTemplate;
        }

        throw new InvalidOperationException("SlideViewTemplateSelector requires TrackTemplate or SkeletonTemplate.");
    }
}
