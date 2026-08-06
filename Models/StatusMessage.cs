namespace mashin.Models;

public readonly record struct StatusMessage(
    string Text,
    bool IsLoading = false,
    TimeSpan? Duration = null)
{
    public static readonly StatusMessage None = new(string.Empty);
    public static readonly StatusMessage Offline = new("Offline");
    public static readonly StatusMessage Connecting = new("Verbinde zum Server", IsLoading: true);
    public static readonly StatusMessage Connected = new("Verbunden", Duration: TimeSpan.FromSeconds(3));
    public static readonly StatusMessage Login = new("Login", IsLoading: true);
    public static readonly StatusMessage LoginSuccessful = new("Login erfolgreich", Duration: TimeSpan.FromSeconds(3));
}