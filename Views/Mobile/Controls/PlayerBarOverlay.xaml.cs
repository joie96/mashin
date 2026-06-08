namespace mashin.Views.Mobile.Controls;

public partial class PlayerBarOverlay : ContentView
{
    #region Constants

    private const double DragDismissThreshold = 100d;
    private const double QueueOpenThreshold = 90d;
    private const double QueueOpenReleaseThreshold = 140d;
    private const double DragResistance = 0.92d;
    private const double CoverHeightFactor = 0.5d;
    private const uint BackdropInDurationMs = 220;
    private const uint BackdropOutDurationMs = 180;
    private const uint InSlideDurationMs = 320;
    private const uint OutSlideDurationMs = 260;
    private const uint DragCancelSnapBackDurationMs = 180;

    #endregion

    #region Fields

    private bool _isSheetDragging;
    private bool _canDismissForCurrentPan;
    private bool _dismissTriggered;
    private bool _queueDragStarted;
    private double _queueDragDistance;

    #endregion

    #region State

    public bool IsOpen { get; private set; }

    public bool IsAnimating { get; private set; }

    #endregion

    #region Events

    public event EventHandler? DismissRequested;

    #endregion

    #region Construction

    public PlayerBarOverlay()
    {
        InitializeComponent();
        WireCoverHeightUpdates();
    }

    #endregion

    #region Public API

    public Task ShowAsync() => AnimateInAsync();

    public Task HideAsync() => AnimateOutAsync();

    #endregion

    #region Overlay Animation

    public async Task AnimateInAsync()
    {
        if (IsOpen || IsAnimating)
        {
            return;
        }

        IsAnimating = true;
        try
        {
            _dismissTriggered = false;
            Backdrop.Opacity = 0;
            OverlaySheet.TranslationY = GetSlideDistance();

            await Task.WhenAll(
                Backdrop.FadeToAsync(1, BackdropInDurationMs, Easing.SinOut),
                OverlaySheet.TranslateToAsync(0, 0, InSlideDurationMs, Easing.SinOut));

            IsOpen = true;
        }
        finally
        {
            IsAnimating = false;
        }
    }

    public async Task AnimateOutAsync()
    {
        if (!IsOpen || IsAnimating)
        {
            return;
        }

        IsAnimating = true;
        try
        {
            _dismissTriggered = true;
            var slideDistance = GetSlideDistance();

            await Task.WhenAll(
                Backdrop.FadeToAsync(0, BackdropOutDurationMs, Easing.SinIn),
                OverlaySheet.TranslateToAsync(0, slideDistance, OutSlideDurationMs, Easing.SinIn));

            IsOpen = false;
        }
        finally
        {
            IsAnimating = false;
        }
    }

    #endregion

    #region Gesture Handlers

    private void OnBackdropTapped(object? sender, TappedEventArgs e)
    {
        DismissRequested?.Invoke(this, EventArgs.Empty);
    }

    private void OnSheetPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        HandlePanUpdated(e);
    }

    private void HandlePanUpdated(PanUpdatedEventArgs e)
    {
        switch (e.StatusType)
        {
            case GestureStatus.Started:
                _canDismissForCurrentPan = IsOpen && !IsAnimating;
                _isSheetDragging = _canDismissForCurrentPan;
                _queueDragStarted = false;
                _queueDragDistance = 0;
                break;

            case GestureStatus.Running when _isSheetDragging:
                if (_dismissTriggered || !_canDismissForCurrentPan)
                {
                    break;
                }

                // Keep horizontal gestures (e.g. slider scrubbing) from triggering dismiss.
                if (Math.Abs(e.TotalX) > Math.Abs(e.TotalY))
                {
                    return;
                }

                if (e.TotalY < 0)
                {
                    var upwardPullDistance = Math.Abs(e.TotalY) * DragResistance;

                    if (!_queueDragStarted && upwardPullDistance >= QueueOpenThreshold)
                    {
                        BeginQueueOverlaySwipeDrag();
                        _queueDragStarted = true;
                    }

                    if (_queueDragStarted)
                    {
                        _queueDragDistance = upwardPullDistance;
                        UpdateQueueOverlaySwipeDrag(_queueDragDistance);
                    }

                    return;
                }

                if (e.TotalY <= 0)
                {
                    return;
                }

                var translationY = Math.Max(0, e.TotalY * DragResistance);
                OverlaySheet.TranslationY = translationY;

                if (translationY >= DragDismissThreshold)
                {
                    _dismissTriggered = true;
                    DismissRequested?.Invoke(this, EventArgs.Empty);
                }

                break;

            case GestureStatus.Completed:
            case GestureStatus.Canceled:
                if (_queueDragStarted)
                {
                    var shouldOpenQueue = _queueDragDistance >= QueueOpenReleaseThreshold;
                    _ = EndQueueOverlaySwipeDragAsync(shouldOpenQueue);
                }

                if (!_dismissTriggered && OverlaySheet.TranslationY > 0)
                {
                    _ = OverlaySheet.TranslateToAsync(0, 0, DragCancelSnapBackDurationMs, Easing.SinOut);
                }

                _isSheetDragging = false;
                _canDismissForCurrentPan = false;
                _queueDragStarted = false;
                _queueDragDistance = 0;
                break;
        }
    }

    #endregion

    #region Queue Overlay

    private async void OnQueueButtonTapped(object? sender, TappedEventArgs e)
    {
        var overlayService = ResolveOverlayService();
        if (overlayService == null || BindingContext == null)
        {
            return;
        }

        if (overlayService.IsQueueOverlayAnimating)
        {
            return;
        }

        if (overlayService.IsQueueOverlayOpen)
        {
            await overlayService.HideQueueOverlayAsync();
            return;
        }

        await overlayService.ShowQueueOverlayAsync(BindingContext);
    }

    private void BeginQueueOverlaySwipeDrag()
    {
        var overlayService = ResolveOverlayService();
        if (overlayService == null || BindingContext == null)
        {
            return;
        }

        if (overlayService.IsQueueOverlayAnimating || overlayService.IsQueueOverlayOpen)
        {
            return;
        }

        overlayService.BeginQueueOverlayInteractiveOpen(BindingContext);
    }

    private void UpdateQueueOverlaySwipeDrag(double upwardPullDistance)
    {
        var overlayService = ResolveOverlayService();
        if (overlayService == null)
        {
            return;
        }

        overlayService.UpdateQueueOverlayInteractiveOpen(upwardPullDistance);
    }

    private async Task EndQueueOverlaySwipeDragAsync(bool shouldOpenQueue)
    {
        var overlayService = ResolveOverlayService();
        if (overlayService == null)
        {
            return;
        }

        if (!overlayService.IsQueueOverlayInteractiveOpening)
        {
            return;
        }

        await overlayService.EndQueueOverlayInteractiveOpenAsync(shouldOpenQueue);
    }

    #endregion

    #region Layout Sizing

    private void WireCoverHeightUpdates()
    {
        SizeChanged += OnOverlaySizeChanged;
        OverlaySheet.SizeChanged += OnOverlaySizeChanged;
        CoverArtBorder.SizeChanged += OnOverlaySizeChanged;
    }

    private void OnOverlaySizeChanged(object? sender, EventArgs e)
    {
        UpdateCoverHeight();
    }

    private void UpdateCoverHeight()
    {
        if (OverlaySheet.Height <= 0 || OverlaySheet.Width <= 0)
        {
            return;
        }

        var maxByHeight = OverlaySheet.Height * CoverHeightFactor;
        var fullWidthSide = Math.Max(0, OverlaySheet.Width - CoverArtBorder.Margin.Left - CoverArtBorder.Margin.Right);
        var targetSide = Math.Min(fullWidthSide, maxByHeight);

        if (targetSide <= 0)
        {
            return;
        }

        var canUseFullWidth = fullWidthSide <= maxByHeight + 0.5;

        if (canUseFullWidth)
        {
            if (CoverArtBorder.WidthRequest >= 0)
            {
                CoverArtBorder.WidthRequest = -1;
                CoverArtBorder.HorizontalOptions = LayoutOptions.Fill;
            }
        }
        else
        {
            if (Math.Abs(CoverArtBorder.WidthRequest - targetSide) >= 0.5)
            {
                CoverArtBorder.WidthRequest = targetSide;
            }

            if (!CoverArtBorder.HorizontalOptions.Equals(LayoutOptions.Center))
            {
                CoverArtBorder.HorizontalOptions = LayoutOptions.Center;
            }
        }

        if (Math.Abs(CoverArtBorder.HeightRequest - targetSide) < 0.5)
        {
            return;
        }

        CoverArtBorder.HeightRequest = targetSide;
    }

    #endregion

    #region Helpers

    private double GetSlideDistance()
    {
        var sheetHeight = Height;
        if (sheetHeight <= 0)
        {
            sheetHeight = OverlaySheet.Height > 0 ? OverlaySheet.Height : 760;
        }

        return Math.Max(200, sheetHeight + 28);
    }

    private static mashin.Services.IOverlayService? ResolveOverlayService()
    {
        var services = Application.Current?.Handler?.MauiContext?.Services;
        if (services == null)
        {
            return null;
        }

        return services.GetService(typeof(mashin.Services.IOverlayService)) as mashin.Services.IOverlayService;
    }

    #endregion
}
