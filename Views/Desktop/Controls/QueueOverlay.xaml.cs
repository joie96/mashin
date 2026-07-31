namespace mashin.Views.Desktop.Controls;

public partial class QueueOverlay : ContentView
{
    private const uint AnimationDuration = 240;

    private bool _isAnimating;

    public QueueOverlay()
    {
        InitializeComponent();
    }

    public bool IsOpen { get; private set; }

    public bool IsAnimating => _isAnimating;

    public async Task ShowAsync(int foregroundZIndex)
    {
        if (IsOpen || _isAnimating)
        {
            return;
        }

        _isAnimating = true;

        try
        {
            IsVisible = true;
            ZIndex = foregroundZIndex;
            OverlayBackdrop.Opacity = 0;

            var hostHeight = Height;
            if (hostHeight <= 0)
            {
                hostHeight = Application.Current?.Windows.Count > 0
                    ? Application.Current.Windows[0].Height
                    : 1000;
            }

            OverlayPanel.TranslationY = hostHeight;

            await Task.WhenAll(
                OverlayBackdrop.FadeToAsync(1, AnimationDuration, Easing.CubicOut),
                OverlayPanel.TranslateToAsync(0, 0, AnimationDuration, Easing.CubicOut));

            IsOpen = true;
        }
        finally
        {
            _isAnimating = false;
        }
    }

    public async Task HideAsync(int backgroundZIndex)
    {
        if (!IsOpen || _isAnimating)
        {
            return;
        }

        _isAnimating = true;

        try
        {
            var hostHeight = Height;
            if (hostHeight <= 0)
            {
                hostHeight = Application.Current?.Windows.Count > 0
                    ? Application.Current.Windows[0].Height
                    : 1000;
            }

            await Task.WhenAll(
                OverlayBackdrop.FadeToAsync(0, AnimationDuration, Easing.CubicOut),
                OverlayPanel.TranslateToAsync(0, hostHeight, AnimationDuration, Easing.CubicIn));

            // Keep the overlay rendered for warm state, move it behind and reset panel position.
            ZIndex = backgroundZIndex;
            OverlayPanel.TranslationY = 0;
            IsOpen = false;
        }
        finally
        {
            _isAnimating = false;
        }
    }
}