namespace mashin.Views.Mobile.Controls;

public partial class PlayerBarOverlay : ContentView
{
    private const double DragDismissThreshold = 100d;
    private const double DragResistance = 0.92d;
    private const double CoverHeightFactor = 0.5d;
    private const uint BackdropInDurationMs = 220;
    private const uint BackdropOutDurationMs = 180;
    private const uint InSlideDurationMs = 320;
    private const uint OutSlideDurationMs = 260;
    private const uint DragCancelSnapBackDurationMs = 180;

    private bool _isSheetDragging;
    private bool _canDismissForCurrentPan;
    private bool _dismissTriggered;

    public bool IsOpen { get; private set; }

    public bool IsAnimating { get; private set; }

    public event EventHandler? DismissRequested;

    public PlayerBarOverlay()
    {
        InitializeComponent();
        WireCoverHeightUpdates();
    }

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

    public Task ShowAsync() => AnimateInAsync();

    public Task HideAsync() => AnimateOutAsync();

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
                if (!_dismissTriggered)
                {
                    _ = OverlaySheet.TranslateToAsync(0, 0, DragCancelSnapBackDurationMs, Easing.SinOut);
                }

                _isSheetDragging = false;
                _canDismissForCurrentPan = false;
                break;
        }
    }

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

    private double GetSlideDistance()
    {
        var sheetHeight = Height;
        if (sheetHeight <= 0)
        {
            sheetHeight = OverlaySheet.Height > 0 ? OverlaySheet.Height : 760;
        }

        return Math.Max(200, sheetHeight + 28);
    }
}
