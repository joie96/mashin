namespace mashin.Services;

/// <summary>
/// Tracks focus for RowView and TableView to support keyboard shortcuts.
/// </summary>
public static class FocusManager
{
#if WINDOWS
    private static WeakReference<object>? _currentFocusedControl;

    public static void SetFocus(object control)
    {
        _currentFocusedControl = new WeakReference<object>(control);
    }

    public static bool HasFocus(object control)
    {
        if (_currentFocusedControl?.TryGetTarget(out var focused) == true)
        {
            var hasFocus = ReferenceEquals(focused, control);
            return hasFocus;
        }
        return false;
    }
#else
    public static void SetFocus(object control) { }
    public static bool HasFocus(object control) => true;
#endif
}