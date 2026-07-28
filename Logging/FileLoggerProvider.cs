using System.Collections.Concurrent;
using Microsoft.Extensions.Logging;

namespace mashin.Logging;

public sealed class FileLoggerProvider : ILoggerProvider
{
    private readonly string _filePath;
    private readonly object _writeLock = new();
    private readonly ConcurrentDictionary<string, FileLogger> _loggers = new(StringComparer.Ordinal);
    private StreamWriter? _writer;
    private bool _disposed;

    public FileLoggerProvider(string filePath)
    {
        _filePath = filePath;
        InitializeWriter();
    }

    public ILogger CreateLogger(string categoryName)
    {
        return _loggers.GetOrAdd(categoryName, category => new FileLogger(category, this));
    }

    public void Dispose()
    {
        lock (_writeLock)
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
            _writer?.Dispose();
            _writer = null;
        }
    }

    internal void WriteLog(LogLevel level, string category, EventId eventId, string message, Exception? exception)
    {
        lock (_writeLock)
        {
            if (_disposed)
            {
                return;
            }

            if (_writer == null)
            {
                return;
            }

            var timestamp = DateTimeOffset.Now.ToString("yyyy-MM-dd HH:mm:ss.fff");
            _writer.Write('[');
            _writer.Write(timestamp);
            _writer.Write("] [");
            _writer.Write(ToShortLevel(level));
            _writer.Write("] ");
            _writer.Write(category);
            _writer.Write(": ");
            _writer.Write(message.Replace(Environment.NewLine, " "));

            if (exception is not null)
            {
                _writer.Write(" | ");
                _writer.Write(exception.ToString().Replace(Environment.NewLine, " "));
            }

            if (eventId.Id != 0)
            {
                _writer.Write(" | EventId=");
                _writer.Write(eventId.Id);
            }

            _writer.WriteLine();
        }
    }

    private void InitializeWriter()
    {
        var directory = Path.GetDirectoryName(_filePath);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }

        _writer = new StreamWriter(new FileStream(_filePath, FileMode.Append, FileAccess.Write, FileShare.ReadWrite))
        {
            AutoFlush = true
        };
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

    private sealed class FileLogger : ILogger
    {
        private readonly string _category;
        private readonly FileLoggerProvider _provider;

        public FileLogger(string category, FileLoggerProvider provider)
        {
            _category = category;
            _provider = provider;
        }

        public IDisposable BeginScope<TState>(TState state) where TState : notnull
        {
            return NullScope.Instance;
        }

        public bool IsEnabled(LogLevel logLevel)
        {
            return logLevel != LogLevel.None;
        }

        public void Log<TState>(LogLevel logLevel, EventId eventId, TState state, Exception? exception, Func<TState, Exception?, string> formatter)
        {
            if (!IsEnabled(logLevel) || formatter is null)
            {
                return;
            }

            var message = formatter(state, exception);
            if (string.IsNullOrWhiteSpace(message) && exception is null)
            {
                return;
            }

            _provider.WriteLog(logLevel, _category, eventId, message, exception);
        }
    }

    private sealed class NullScope : IDisposable
    {
        public static NullScope Instance { get; } = new();

        public void Dispose()
        {
        }
    }
}
