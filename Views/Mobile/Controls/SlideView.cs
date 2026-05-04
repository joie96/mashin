using mashin.Models;
using mashin.Services;
using System.Windows.Input;

namespace mashin.Views.Mobile.Controls;

public partial class SlideView : ContentView
{
    #region Bindable Properties

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

    #region CLR Wrappers

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

    #region Lifecycle

    public SlideView()
    {
        InitializeComponent();
    }

    #endregion

    #region UI Events

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
