using System;

#if WINDOWS
using Microsoft.UI.Xaml.Input;
using Windows.System;
#endif

namespace mashin.Services;

public interface IKeyboardService
{
    /// <summary>
    /// Indicates if Control key is currently pressed.
    /// </summary>
    bool IsControlPressed { get; }

    /// <summary>
    /// Indicates if Shift key is currently pressed.
    /// </summary>
    bool IsShiftPressed { get; }

    /// <summary>
    /// Indicates if Alt key is currently pressed.
    /// </summary>
    bool IsAltPressed { get; }

    /// <summary>
    /// Raised when a relevant key action is detected (shortcuts or single keys).
    /// </summary>
    event EventHandler<KeyActionEventArgs>? KeyActionDetected;
}

public class KeyActionEventArgs : EventArgs
{
    public KeyAction Action { get; }
    public object? Context { get; }

    public KeyActionEventArgs(KeyAction action, object? context = null)
    {
        Action = action;
        Context = context;
    }
}

public enum KeyAction
{
    CtrlA,
    CtrlC,
    CtrlV,
    CtrlX,
    CtrlZ,
    Escape,
    Delete,
    Enter,
    Space
}

#if WINDOWS
/// <summary>
/// Windows-native keyboard state tracking and shortcut detection.
/// </summary>
public class WindowsKeyboardService : IKeyboardService
{
    private bool _isControlPressed;
    private bool _isShiftPressed;
    private bool _isAltPressed;
    private KeyEventHandler? _keyDownHandler;
    private KeyEventHandler? _keyUpHandler;
    private Microsoft.UI.Xaml.Window? _window;

    public bool IsControlPressed => _isControlPressed;
    public bool IsShiftPressed => _isShiftPressed;
    public bool IsAltPressed => _isAltPressed;

    public event EventHandler<KeyActionEventArgs>? KeyActionDetected;

    public WindowsKeyboardService()
    {
        AttachToWindow();
    }

    private void AttachToWindow()
    {
        var mauiWindow = Microsoft.Maui.Controls.Application.Current?.Windows.FirstOrDefault();
        _window = mauiWindow?.Handler?.PlatformView as Microsoft.UI.Xaml.Window;

        if (_window?.Content == null)
        {
            return;
        }

        _keyDownHandler = (_, args) =>
        {
            UpdateModifierStates();

            // Detect key combinations and actions
            if (_isControlPressed)
            {
                switch (args.Key)
                {
                    case VirtualKey.A:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.CtrlA));
                        args.Handled = true;
                        break;
                    case VirtualKey.C:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.CtrlC));
                        break;
                    case VirtualKey.V:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.CtrlV));
                        break;
                    case VirtualKey.X:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.CtrlX));
                        break;
                    case VirtualKey.Z:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.CtrlZ));
                        break;
                }
            }
            else
            {
                // Single key actions
                switch (args.Key)
                {
                    case VirtualKey.Escape:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.Escape));
                        args.Handled = true;
                        break;
                    case VirtualKey.Delete:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.Delete));
                        break;
                    case VirtualKey.Enter:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.Enter));
                        break;
                    case VirtualKey.Space:
                        KeyActionDetected?.Invoke(this, new KeyActionEventArgs(KeyAction.Space));
                        break;
                }
            }
        };

        _keyUpHandler = (_, args) =>
        {
            UpdateModifierStates();
        };

        _window.Content.KeyDown += _keyDownHandler;
        _window.Content.KeyUp += _keyUpHandler;
    }

    private void UpdateModifierStates()
    {
        var ctrlState = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control);
        var shiftState = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift);
        var altState = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Menu);

        _isControlPressed = (ctrlState & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;
        _isShiftPressed = (shiftState & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;
        _isAltPressed = (altState & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;
    }

    ~WindowsKeyboardService()
    {
        Detach();
    }

    private void Detach()
    {
        if (_window?.Content != null)
        {
            if (_keyDownHandler != null)
            {
                _window.Content.KeyDown -= _keyDownHandler;
            }
            if (_keyUpHandler != null)
            {
                _window.Content.KeyUp -= _keyUpHandler;
            }
        }

        _keyDownHandler = null;
        _keyUpHandler = null;
        _window = null;
    }
}

#else
/// <summary>
/// Default keyboard service for non-Windows platforms.
/// </summary>
public class DefaultKeyboardService : IKeyboardService
{
    public bool IsControlPressed => false;
    public bool IsShiftPressed => false;
    public bool IsAltPressed => false;

    public event EventHandler<KeyActionEventArgs>? KeyActionDetected;
}
#endif