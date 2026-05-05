using mashin.Models;
using mashin.Services;
using Microsoft.Extensions.DependencyInjection;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class SlideView : ContentView
{
    #region Bindable properties

    public static readonly BindableProperty ItemsSourceProperty =
        BindableProperty.Create(nameof(ItemsSource), typeof(IEnumerable<object>), typeof(SlideView));

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
        BindableProperty.Create(nameof(CoverSize), typeof(double), typeof(SlideView), 145d);

    public static readonly BindableProperty ItemCornerRadiusProperty =
        BindableProperty.Create(nameof(ItemCornerRadius), typeof(float), typeof(SlideView), 8f);

    public static readonly BindableProperty ItemWidthProperty =
        BindableProperty.Create(nameof(ItemWidth), typeof(double), typeof(SlideView), 320d);

    #endregion

    #region Fields

    private IArtworkService? _artworkService;

    #endregion

    #region Properties

    private IArtworkService? ArtworkService =>
        _artworkService ??=
            IPlatformApplication.Current?.Services.GetService<IArtworkService>() ??
            Handler?.MauiContext?.Services.GetService<IArtworkService>();

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

    public double ItemWidth
    {
        get => (double)GetValue(ItemWidthProperty);
        set => SetValue(ItemWidthProperty, value);
    }

    #endregion

    #region Construction

    public SlideView()
    {
        InitializeComponent();
    }

    #endregion

    #region UI Events

    private async void OnTrackBackgroundBindingContextChanged(object? sender, EventArgs e)
    {
        if (sender is not Image backgroundImage)
        {
            return;
        }

        if (backgroundImage.BindingContext is not MediaItem mediaItem)
        {
            backgroundImage.Source = null;
            return;
        }

        await ApplyTrackBackgroundAsync(backgroundImage, mediaItem);
    }

    private async Task ApplyTrackBackgroundAsync(Image backgroundImage, MediaItem mediaItem, int attempt = 0)
    {
        if (!ReferenceEquals(backgroundImage.BindingContext, mediaItem))
        {
            return;
        }

        var imageUrl = mediaItem.ImageUrl;
        if (string.IsNullOrWhiteSpace(imageUrl))
        {
            backgroundImage.Source = null;
            return;
        }

        // Keep card readable while blurred image is generated.
        backgroundImage.Source = imageUrl;

        var artworkService = ArtworkService;
        if (artworkService == null)
        {
            if (attempt < 3)
            {
                await Task.Delay(120);
                await ApplyTrackBackgroundAsync(backgroundImage, mediaItem, attempt + 1);
            }

            return;
        }

        var blurredSource = await artworkService.GetBlurredCoverSourceAsync(imageUrl);

        await MainThread.InvokeOnMainThreadAsync(() =>
        {
            if (!ReferenceEquals(backgroundImage.BindingContext, mediaItem))
            {
                return;
            }

            if (blurredSource != null)
            {
                backgroundImage.Source = blurredSource;
            }
        });
    }

    private async void OnPlayOverlayClicked(object? sender, TappedEventArgs e)
    {
        if (sender is not BindableObject { BindingContext: MediaItem item } || MediaActions == null)
        {
            return;
        }

        await MediaActions.PlayMediaAsync(item);
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
