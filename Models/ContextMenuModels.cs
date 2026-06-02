using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Text;
using System.Windows.Input;
using MauiIcons.Fluent;
using MauiIcons.Fluent.Filled;

namespace mashin.Models;

/// <summary>
/// Model for a context menu item with support for icons, shortcuts, and submenus.
/// </summary>
public class ContextMenuItem : BindableObject
{
    private ObservableCollection<ContextMenuItem> _subItems = new();

    /// <summary>
    /// Display text of the menu item.
    /// </summary>
    public string Text { get; set; } = string.Empty;

    /// <summary>
    /// Icon as a MauiIcons enum value (e.g., FluentIcons.Add).
    /// </summary>
    public object? Icon { get; set; }

    /// <summary>
    /// Indicates that the icon comes from FluentFilledIcons.
    /// </summary>
    public bool IconIsFilled { get; set; }

    /// <summary>
    /// Keyboard shortcut (e.g., "Ctrl+A", "Shift+Delete").
    /// </summary>
    public string KeyboardShortcut { get; set; } = string.Empty;

    /// <summary>
    /// Command executed when the menu item is clicked.
    /// </summary>
    public ICommand? Command { get; set; }

    /// <summary>
    /// Optional: parameter for the command.
    /// </summary>
    public object? CommandParameter { get; set; }

    /// <summary>
    /// Indicates whether this item is a visual separator.
    /// </summary>
    public bool IsSeparator { get; set; }

    /// <summary>
    /// Indicates whether the menu item is enabled (grayed out when false).
    /// </summary>
    public bool IsEnabled { get; set; } = true;

    /// <summary>
    /// Submenu items (for nested menus).
    /// </summary>
    public ObservableCollection<ContextMenuItem> SubItems
    {
        get => _subItems;
        set
        {
            if (_subItems != value)
            {
                _subItems = value;
                OnPropertyChanged();
                OnPropertyChanged(nameof(HasSubItems));
                OnPropertyChanged(nameof(HasKeyboardShortcut));
            }
        }
    }

    /// <summary>
    /// Indicates whether an icon is present.
    /// </summary>
    public bool HasIcon => Icon != null;

    /// <summary>
    /// Returns true when Icon is a regular Fluent icon enum value.
    /// </summary>
    public bool HasFluentIcon => Icon is FluentIcons;

    /// <summary>
    /// Returns true when Icon is a filled Fluent icon enum value.
    /// </summary>
    public bool HasFluentFilledIcon => Icon is FluentFilledIcons;

    /// <summary>
    /// Typed regular Fluent icon value for XAML bindings.
    /// </summary>
    public FluentIcons FluentIcon => Icon is FluentIcons icon ? icon : default;

    /// <summary>
    /// Typed filled Fluent icon value for XAML bindings.
    /// </summary>
    public FluentFilledIcons FluentFilledIcon => Icon is FluentFilledIcons icon ? icon : default;

    /// <summary>
    /// Indicates whether submenu items are present.
    /// </summary>
    public bool HasSubItems => _subItems?.Count > 0;

    /// <summary>
    /// Indicates whether a keyboard shortcut should be shown
    /// (only when no submenu is present).
    /// </summary>
    public bool HasKeyboardShortcut => !string.IsNullOrEmpty(KeyboardShortcut) && !HasSubItems;
}