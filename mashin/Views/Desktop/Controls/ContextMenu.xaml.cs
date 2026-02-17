using System.Collections.ObjectModel;
using mashin.Models;
using mashin.Services;

namespace mashin.Views.Desktop.Controls;

/// <summary>
/// Custom context menu control that displays items and handles submenus.
/// </summary>
public partial class ContextMenu : ContentView
{
    private IContextMenuService? _contextMenuService;
    private CancellationTokenSource? _hoverCts;
    private CancellationTokenSource? _closeCts;
    private ContextMenuItem? _hoveredItem;
    private ContextMenuItem? _currentSubMenuOwner;
    private bool _isPointerOverSubMenu;
    private bool _isMenuOpen;
    private bool _isSubMenuOpen;

    public static readonly BindableProperty MenuItemsProperty =
        BindableProperty.Create(
            nameof(MenuItems),
            typeof(ObservableCollection<ContextMenuItem>),
            typeof(ContextMenu),
            new ObservableCollection<ContextMenuItem>());

    public ObservableCollection<ContextMenuItem> MenuItems
    {
        get => (ObservableCollection<ContextMenuItem>)GetValue(MenuItemsProperty);
        set => SetValue(MenuItemsProperty, value);
    }

    public event EventHandler<ContextMenuItem>? ItemSelected;
    public event EventHandler? RequestClose;

    public ContextMenu()
    {
        InitializeComponent();

        _contextMenuService = Application.Current?.Handler?.MauiContext?.Services
            .GetService<IContextMenuService>();

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
        ContextSubMenu.PointerOverChanged += OnSubMenuPointerOverChanged;
    }

    protected override void OnHandlerChanged()
    {
        base.OnHandlerChanged();
        _isMenuOpen = Handler != null;
    }

    private void OnLoaded(object? sender, EventArgs e)
    {
        _isMenuOpen = true;
    }

    private void OnUnloaded(object? sender, EventArgs e)
    {
        _isMenuOpen = false;
        _hoverCts?.Cancel();
        _closeCts?.Cancel();
        ContextSubMenu.PointerOverChanged -= OnSubMenuPointerOverChanged;
    }

    private async void OnPointerEntered(object sender, PointerEventArgs e)
    {
        if (sender is Border border && border.BindingContext is ContextMenuItem item)
        {
            HandlePointerOver(border, item);
        }
    }

    private void OnPointerMoved(object sender, PointerEventArgs e)
    {
        if (sender is Border border && border.BindingContext is ContextMenuItem item)
        {
            HandlePointerOver(border, item);
        }
    }

    private void OnPointerExited(object sender, PointerEventArgs e)
    {
        if (sender is Border border)
        {
            border.BackgroundColor = Colors.Transparent;

            if (border.BindingContext is ContextMenuItem item && ReferenceEquals(_hoveredItem, item))
            {
                if (!(_isPointerOverSubMenu && item.HasSubItems))
                {
                    _hoveredItem = null;
                    _hoverCts?.Cancel();
                }
            }

            if (border.BindingContext is ContextMenuItem exitItem && exitItem.HasSubItems && !_isPointerOverSubMenu)
            {
                _closeCts?.Cancel();
                _closeCts = new CancellationTokenSource();
                _ = CloseSubMenuAfterDelayAsync(_closeCts.Token);
            }
        }
    }

    private async void OnMenuItemTapped(object sender, TappedEventArgs e)
    {
        if (sender is Border border && border.BindingContext is ContextMenuItem item)
        {
            if (!item.IsEnabled || item.IsSeparator)
                return;

            // If the item has subitems, open the submenu. Otherwise, execute the command.
            if (item.HasSubItems)
            {
                _hoveredItem = item;
                _hoverCts?.Cancel();
                _closeCts?.Cancel();

                if (_isSubMenuOpen && ReferenceEquals(_currentSubMenuOwner, item))
                {
                    return;
                }

                if (_isSubMenuOpen && !ReferenceEquals(_currentSubMenuOwner, item))
                {
                    _contextMenuService?.CloseSubMenu();
                    _isSubMenuOpen = false;
                    _currentSubMenuOwner = null;
                }

                if (_contextMenuService != null && item.SubItems?.Count > 0)
                {
                    await _contextMenuService.ShowSubMenuAsync(item.SubItems, border);
                    _isSubMenuOpen = true;
                    _currentSubMenuOwner = item;
                }
            }
            else
            {
                item.Command?.Execute(item.CommandParameter);
                ItemSelected?.Invoke(this, item);
                RequestClose?.Invoke(this, EventArgs.Empty);
            }
        }
    }

    private void OnSubMenuPointerOverChanged(object? sender, bool isPointerOver)
    {
        _isPointerOverSubMenu = isPointerOver;

        if (!isPointerOver)
        {
            _closeCts?.Cancel();
            _closeCts = new CancellationTokenSource();
            _ = CloseSubMenuAfterDelayAsync(_closeCts.Token);
        }
    }

    private void HandlePointerOver(Border border, ContextMenuItem item)
    {
        if (!item.IsEnabled)
        {
            return;
        }

        border.BackgroundColor = Application.Current?.Resources.TryGetValue("HoverBackground", out var value) == true && value is Color color ? color : Colors.Transparent;

        if (_isPointerOverSubMenu)
        {
            _isPointerOverSubMenu = false;
        }

        if (ReferenceEquals(_hoveredItem, item))
        {
            return;
        }

        _hoveredItem = item;
        _hoverCts?.Cancel();
        _hoverCts = new CancellationTokenSource();

        if (item.HasSubItems)
        {
            if (_isSubMenuOpen && !ReferenceEquals(_currentSubMenuOwner, item))
            {
                _contextMenuService?.CloseSubMenu();
                _isSubMenuOpen = false;
                _currentSubMenuOwner = null;
            }

            _closeCts?.Cancel();

            _ = OpenSubMenuAfterDelayAsync(item, border, _hoverCts.Token);
        }
        else
        {
            _closeCts?.Cancel();
            _closeCts = new CancellationTokenSource();
            _ = CloseSubMenuAfterDelayAsync(_closeCts.Token);
        }
    }

    private async Task OpenSubMenuAfterDelayAsync(ContextMenuItem item, Border border, CancellationToken token)
    {
        try
        {
            await Task.Delay(200, token);
        }
        catch (TaskCanceledException)
        {
            return;
        }

        if (!ReferenceEquals(_hoveredItem, item))
        {
            return;
        }

        if (!_isMenuOpen)
        {
            return;
        }

        if (_contextMenuService != null && item.SubItems?.Count > 0)
        {
            if (_isSubMenuOpen && ReferenceEquals(_currentSubMenuOwner, item))
            {
                return;
            }

            if (_isSubMenuOpen && !ReferenceEquals(_currentSubMenuOwner, item))
            {
                _contextMenuService?.CloseSubMenu();
                _isSubMenuOpen = false;
                _currentSubMenuOwner = null;
            }

            await _contextMenuService.ShowSubMenuAsync(item.SubItems, border);
            _isSubMenuOpen = true;
            _currentSubMenuOwner = item;
        }
    }

    private async Task CloseSubMenuAfterDelayAsync(CancellationToken token)
    {
        try
        {
            await Task.Delay(250, token);
        }
        catch (TaskCanceledException)
        {
            return;
        }

        if (_isPointerOverSubMenu)
        {
            return;
        }

        _contextMenuService?.CloseSubMenu();
        _isSubMenuOpen = false;
        _currentSubMenuOwner = null;
    }
}