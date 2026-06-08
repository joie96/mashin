namespace mashin.Views.Mobile.Controls;

public partial class QueueOverlay : ContentView
{
    private const double DragDismissThreshold = 100d;
    private const double DragResistance = 0.92d;
    private const uint InSlideDurationMs = 300;
    private const uint OutSlideDurationMs = 240;
    private const uint DragCancelSnapBackDurationMs = 180;

    private bool _isSheetDragging;
    private bool _canDismissForCurrentPan;
    private bool _dismissTriggered;
    private double _queueVerticalOffset;

    public QueueOverlay()
    {
        InitializeComponent();

        QueueItemsTable.ItemsPanUpdated += OnQueueItemsPanUpdated;
        QueueItemsTable.VerticalOffsetChanged += OnQueueItemsVerticalOffsetChanged;
    }

    public bool IsOpen { get; private set; }

    public bool IsAnimating { get; private set; }

    public event EventHandler? DismissRequested;

    public async Task ShowAsync()
    {
        if (IsOpen || IsAnimating)
        {
            return;
        }

        IsAnimating = true;
        try
        {
            _dismissTriggered = false;
            IsVisible = true;
            OverlaySheet.TranslationY = GetSlideDistance();

            // Let the view render one frame at its start position before animating in.
            await Task.Yield();

            await OverlaySheet.TranslateToAsync(0, 0, InSlideDurationMs, Easing.SinOut);

            IsOpen = true;
        }
        finally
        {
            IsAnimating = false;
        }
    }

    public async Task HideAsync()
    {
        if (!IsOpen || IsAnimating)
        {
            return;
        }

        IsAnimating = true;
        try
        {
            _dismissTriggered = true;
            await OverlaySheet.TranslateToAsync(0, GetSlideDistance(), OutSlideDurationMs, Easing.SinIn);

            IsVisible = false;
            IsOpen = false;
        }
        finally
        {
            IsAnimating = false;
        }
    }

    private void OnSheetPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        HandlePanUpdated(e);
    }

    private void OnQueueItemsPanUpdated(object? sender, PanUpdatedEventArgs e)
    {
        HandlePanUpdated(e);
    }

    private void OnQueueItemsVerticalOffsetChanged(object? sender, double verticalOffset)
    {
        _queueVerticalOffset = verticalOffset;
    }

    private void HandlePanUpdated(PanUpdatedEventArgs e)
    {
        switch (e.StatusType)
        {
            case GestureStatus.Started:
                _canDismissForCurrentPan = IsOpen && !IsAnimating && IsScrollAtTop();
                _isSheetDragging = _canDismissForCurrentPan;
                break;

            case GestureStatus.Running when _isSheetDragging:
                if (_dismissTriggered || !_canDismissForCurrentPan)
                {
                    break;
                }

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

    private bool IsScrollAtTop()
    {
        return _queueVerticalOffset <= 0d;
    }

    private double GetSlideDistance()
    {
        var sheetHeight = Height;
        if (sheetHeight <= 0)
        {
            sheetHeight = OverlaySheet.Height > 0 ? OverlaySheet.Height : 760;
        }

        return Math.Max(200, sheetHeight + 24);
    }
}
