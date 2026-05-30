namespace mashin.Services;

/// <summary>
/// Tracks focus for RowView and TableView to support keyboard shortcuts.
/// </summary>
public static class FocusManager
{
    private static WeakReference<object>? _currentFocusedControl;

    public static void SetFocus(object control)
    {
        _currentFocusedControl = new WeakReference<object>(control);
    }

    public static bool HasFocus(object control)
    {
        if (_currentFocusedControl?.TryGetTarget(out var focused) == true)
        {
            return ReferenceEquals(focused, control);
        }

        return false;
    }

    public static bool GetFocusedControl<TControl>(out TControl? control) where TControl : class
    {
        control = null;

        if (_currentFocusedControl?.TryGetTarget(out var focused) != true)
        {
            return false;
        }

        control = focused as TControl;
        return control != null;
    }
}