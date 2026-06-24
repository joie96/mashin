using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Console;
using Microsoft.Extensions.Options;

namespace mashin.Logging;

public sealed class CustomConsoleFormatter : ConsoleFormatter, IDisposable
{
    public const string FormatterName = "Custom";

    private readonly IDisposable? _optionsReloadToken;
    private SimpleConsoleFormatterOptions _options;

    public CustomConsoleFormatter(IOptionsMonitor<SimpleConsoleFormatterOptions> options)
        : base(FormatterName)
    {
        _options = options.CurrentValue;
        _optionsReloadToken = options.OnChange(updated => _options = updated);
    }

    public void Dispose()
    {
        _optionsReloadToken?.Dispose();
    }

    private static string ToShortLevel(LogLevel level)
    {
        return level switch
        {
            LogLevel.Trace => "TRC",
            LogLevel.Debug => "DBG",
            LogLevel.Information => "INF",
            LogLevel.Warning => "WRN",
            LogLevel.Error => "ERR",
            LogLevel.Critical => "CRT",
            LogLevel.None => "NON",
            _ => "UNK"
        };
    }

    public override void Write<TState>(in Microsoft.Extensions.Logging.Abstractions.LogEntry<TState> logEntry, IExternalScopeProvider? scopeProvider, TextWriter textWriter)
    {
        var formatter = logEntry.Formatter;
        if (formatter is null)
        {
            return;
        }

        var message = formatter(logEntry.State, logEntry.Exception);
        if (string.IsNullOrEmpty(message) && logEntry.Exception is null)
        {
            return;
        }

        var timestamp = string.IsNullOrWhiteSpace(_options.TimestampFormat)
            ? DateTimeOffset.Now.ToString("HH:mm:ss.fff")
            : DateTimeOffset.Now.ToString(_options.TimestampFormat);

        var level = ToShortLevel(logEntry.LogLevel);
        var category = logEntry.Category ?? "Unknown";

        textWriter.Write('[');
        textWriter.Write(timestamp);
        textWriter.Write("] [");
        textWriter.Write(level);
        textWriter.Write("] ");
        textWriter.Write(category);
        textWriter.Write(":");

        if (!string.IsNullOrEmpty(message))
        {
            textWriter.Write(' ');
            textWriter.Write(message.Replace(Environment.NewLine, " "));
        }

        if (logEntry.Exception is not null)
        {
            textWriter.Write(' ');
            textWriter.Write(logEntry.Exception.ToString().Replace(Environment.NewLine, " "));
        }

        textWriter.WriteLine();
    }
}
