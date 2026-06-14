using System.Collections.Concurrent;
using System.Net.WebSockets;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading.Channels;
using mashin.Converters;
using mashin.Models;
using Microsoft.Extensions.Logging;

namespace mashin.Services;

public sealed record MusicAssistantEvent(
    string Event,
    string? ObjectId,
    JsonElement Data,
    DateTimeOffset ReceivedAt);

public sealed record MusicAssistantQueueEvent(
    string Event,
    string? QueueId,
    PlayerQueue? Queue,
    double? ElapsedTimeSeconds,
    DateTimeOffset ReceivedAt);

public interface IMusicAssistantEventHub : IAsyncDisposable
{
    #region Interface Core
    bool IsConnected { get; }

    event EventHandler<MusicAssistantEvent>? EventReceived;

    Task StartAsync(CancellationToken cancellationToken = default);

    Task StopAsync(CancellationToken cancellationToken = default);

    IAsyncEnumerable<MusicAssistantEvent> Events(CancellationToken cancellationToken = default);
    #endregion

    #region Interface Queue
    event EventHandler<MusicAssistantQueueEvent>? QueueEventReceived;

    IAsyncEnumerable<MusicAssistantQueueEvent> QueueEvents(CancellationToken cancellationToken = default);
    #endregion
}

public sealed class MusicAssistantEventHub : IMusicAssistantEventHub
{
    #region Constants
    private static readonly HashSet<string> QueueEventsFilter = new(StringComparer.OrdinalIgnoreCase)
    {
        "queue_added",
        "queue_updated",
        "queue_items_updated",
        "queue_time_updated",
        "queue_settings_updated"
    };

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        Converters =
        {
            new JsonStringEnumConverter(),
            new FlexibleIntConverter(),
            new MediaItemJsonConverter()
        }
    };
    #endregion

    #region Fields
    private readonly SettingsService _settings;
    private readonly ILogger<MusicAssistantEventHub> _logger;
    private readonly SemaphoreSlim _lifecycleGate = new(1, 1);
    private readonly ConcurrentDictionary<Guid, Channel<MusicAssistantEvent>> _eventSubscribers = new();
    private readonly ConcurrentDictionary<Guid, Channel<MusicAssistantQueueEvent>> _queueSubscribers = new();

    private CancellationTokenSource? _runCts;
    private Task? _runTask;
    private ClientWebSocket? _socket;
    #endregion

    #region Construction

    public MusicAssistantEventHub(
        SettingsService settings,
        ILogger<MusicAssistantEventHub> logger)
    {
        _settings = settings;
        _logger = logger;
    }
    #endregion

    #region Core API
    public bool IsConnected { get; private set; }

    public event EventHandler<MusicAssistantEvent>? EventReceived;

    public async Task StartAsync(CancellationToken cancellationToken = default)
    {
        await _lifecycleGate.WaitAsync(cancellationToken);
        try
        {
            if (_runTask is { IsCompleted: false })
            {
                return;
            }

            _runCts = new CancellationTokenSource();
            _runTask = Task.Run(() => RunAsync(_runCts.Token), CancellationToken.None);
            _logger.LogInformation("Music Assistant event hub started.");
        }
        finally
        {
            _lifecycleGate.Release();
        }
    }

    public async Task StopAsync(CancellationToken cancellationToken = default)
    {
        CancellationTokenSource? cts;
        Task? runTask;
        ClientWebSocket? socket;

        await _lifecycleGate.WaitAsync(cancellationToken);
        try
        {
            cts = _runCts;
            runTask = _runTask;
            socket = _socket;

            _runCts = null;
            _runTask = null;
            _socket = null;
            IsConnected = false;
        }
        finally
        {
            _lifecycleGate.Release();
        }

        if (cts != null)
        {
            cts.Cancel();
            cts.Dispose();
        }

        if (socket != null)
        {
            await CloseSocketAsync(socket, CancellationToken.None);
            socket.Dispose();
        }

        if (runTask != null)
        {
            try
            {
                await runTask;
            }
            catch (OperationCanceledException)
            {
                // Expected on shutdown.
            }
        }

        _logger.LogInformation("Music Assistant event hub stopped.");
    }

    public async ValueTask DisposeAsync()
    {
        await StopAsync();

        foreach (var channel in _eventSubscribers.Values)
        {
            channel.Writer.TryComplete();
        }

        foreach (var channel in _queueSubscribers.Values)
        {
            channel.Writer.TryComplete();
        }

        _eventSubscribers.Clear();
        _queueSubscribers.Clear();
        _lifecycleGate.Dispose();
    }

    public async IAsyncEnumerable<MusicAssistantEvent> Events(
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        var subscriberId = Guid.NewGuid();
        var channel = Channel.CreateUnbounded<MusicAssistantEvent>(new UnboundedChannelOptions
        {
            SingleReader = true,
            SingleWriter = false
        });

        _eventSubscribers[subscriberId] = channel;

        try
        {
            await foreach (var hubEvent in channel.Reader.ReadAllAsync(cancellationToken))
            {
                yield return hubEvent;
            }
        }
        finally
        {
            _eventSubscribers.TryRemove(subscriberId, out var removedChannel);
            removedChannel?.Writer.TryComplete();
        }
    }
    #endregion

    #region Queue Projection
    public event EventHandler<MusicAssistantQueueEvent>? QueueEventReceived;

    public async IAsyncEnumerable<MusicAssistantQueueEvent> QueueEvents(
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        var subscriberId = Guid.NewGuid();
        var channel = Channel.CreateUnbounded<MusicAssistantQueueEvent>(new UnboundedChannelOptions
        {
            SingleReader = true,
            SingleWriter = false
        });

        _queueSubscribers[subscriberId] = channel;

        try
        {
            await foreach (var queueEvent in channel.Reader.ReadAllAsync(cancellationToken))
            {
                yield return queueEvent;
            }
        }
        finally
        {
            _queueSubscribers.TryRemove(subscriberId, out var removedChannel);
            removedChannel?.Writer.TryComplete();
        }
    }

    private bool TryCreateQueueEvent(MusicAssistantEvent hubEvent, out MusicAssistantQueueEvent queueEvent)
    {
        queueEvent = default!;
        if (!QueueEventsFilter.Contains(hubEvent.Event))
        {
            return false;
        }

        PlayerQueue? queue = null;
        double? elapsedTimeSeconds = null;
        if (hubEvent.Data.ValueKind == JsonValueKind.Object)
        {
            queue = JsonSerializer.Deserialize<PlayerQueue>(hubEvent.Data.GetRawText(), JsonOptions);
        }
        else if (string.Equals(hubEvent.Event, "queue_time_updated", StringComparison.OrdinalIgnoreCase)
            && hubEvent.Data.ValueKind == JsonValueKind.Number
            && hubEvent.Data.TryGetDouble(out var elapsedSeconds))
        {
            elapsedTimeSeconds = elapsedSeconds;
        }

        queueEvent = new MusicAssistantQueueEvent(
            Event: hubEvent.Event,
            QueueId: hubEvent.ObjectId,
            Queue: queue,
            ElapsedTimeSeconds: elapsedTimeSeconds,
            ReceivedAt: hubEvent.ReceivedAt);

        return true;
    }

    private void BroadcastQueueEvent(MusicAssistantQueueEvent queueEvent)
    {
        QueueEventReceived?.Invoke(this, queueEvent);

        foreach (var subscriber in _queueSubscribers.Values)
        {
            if (!subscriber.Writer.TryWrite(queueEvent))
            {
                _logger.LogDebug("Dropping queue event for one subscriber due to backpressure.");
            }
        }
    }
    #endregion

    #region WebSocket Transport
    private async Task RunAsync(CancellationToken cancellationToken)
    {
        var retryDelay = TimeSpan.FromSeconds(2);

        while (!cancellationToken.IsCancellationRequested)
        {
            try
            {
                if (string.IsNullOrWhiteSpace(_settings.AuthToken))
                {
                    _logger.LogDebug("Event hub waiting for auth token.");
                    await Task.Delay(TimeSpan.FromSeconds(2), cancellationToken);
                    continue;
                }

                using var socket = new ClientWebSocket();
                _socket = socket;

                var websocketUri = BuildWebSocketUri(_settings.MusicAssistantUrl);
                await socket.ConnectAsync(websocketUri, cancellationToken);

                await AuthenticateAsync(socket, _settings.AuthToken!, cancellationToken);

                IsConnected = true;
                retryDelay = TimeSpan.FromSeconds(2);
                _logger.LogInformation("Connected to Music Assistant event stream: {Uri}", websocketUri);

                await ReceiveLoopAsync(socket, cancellationToken);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                IsConnected = false;
                _logger.LogWarning(ex, "Event hub connection loop error. Retrying in {Delay}s", retryDelay.TotalSeconds);
                await Task.Delay(retryDelay, cancellationToken);
                retryDelay = TimeSpan.FromSeconds(Math.Min(retryDelay.TotalSeconds * 2, 15));
            }
            finally
            {
                IsConnected = false;
            }
        }
    }

    private async Task AuthenticateAsync(ClientWebSocket socket, string token, CancellationToken cancellationToken)
    {
        var authMessageId = $"auth-{Guid.NewGuid():N}";
        var authPayload = JsonSerializer.Serialize(new
        {
            message_id = authMessageId,
            command = "auth",
            args = new { token }
        });

        var authBuffer = Encoding.UTF8.GetBytes(authPayload);
        await socket.SendAsync(authBuffer, WebSocketMessageType.Text, true, cancellationToken);

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(TimeSpan.FromSeconds(10));

        while (!timeoutCts.IsCancellationRequested)
        {
            var responseText = await ReceiveTextMessageAsync(socket, timeoutCts.Token);
            if (string.IsNullOrWhiteSpace(responseText))
            {
                continue;
            }

            using var responseDoc = JsonDocument.Parse(responseText);
            var root = responseDoc.RootElement;

            if (root.TryGetProperty("message_id", out var messageIdElement)
                && string.Equals(messageIdElement.GetString(), authMessageId, StringComparison.Ordinal)
                && root.TryGetProperty("result", out var resultElement)
                && resultElement.TryGetProperty("authenticated", out var authenticatedElement)
                && authenticatedElement.ValueKind == JsonValueKind.True)
            {
                return;
            }

            if (root.TryGetProperty("message_id", out var errorMessageId)
                && string.Equals(errorMessageId.GetString(), authMessageId, StringComparison.Ordinal)
                && root.TryGetProperty("error", out var errorElement))
            {
                throw new InvalidOperationException($"WebSocket auth failed: {errorElement.GetRawText()}");
            }
        }

        throw new TimeoutException("WebSocket auth response timed out.");
    }

    private async Task ReceiveLoopAsync(ClientWebSocket socket, CancellationToken cancellationToken)
    {
        while (socket.State == WebSocketState.Open && !cancellationToken.IsCancellationRequested)
        {
            var responseText = await ReceiveTextMessageAsync(socket, cancellationToken);
            if (string.IsNullOrWhiteSpace(responseText))
            {
                continue;
            }

            try
            {
                using var doc = JsonDocument.Parse(responseText);
                var root = doc.RootElement;

                if (!root.TryGetProperty("event", out var eventElement))
                {
                    continue;
                }

                var eventName = eventElement.GetString();
                if (string.IsNullOrWhiteSpace(eventName))
                {
                    continue;
                }

                var objectId = root.TryGetProperty("object_id", out var objectIdElement)
                    ? objectIdElement.GetString()
                    : null;

                var data = root.TryGetProperty("data", out var dataElement)
                    ? dataElement.Clone()
                    : default;

                var hubEvent = new MusicAssistantEvent(
                    Event: eventName,
                    ObjectId: objectId,
                    Data: data,
                    ReceivedAt: DateTimeOffset.UtcNow);

                BroadcastEvent(hubEvent);

                if (TryCreateQueueEvent(hubEvent, out var queueEvent))
                {
                    BroadcastQueueEvent(queueEvent);
                }
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Failed to parse event message: {Message}", responseText);
            }
        }
    }

    private void BroadcastEvent(MusicAssistantEvent hubEvent)
    {
        EventReceived?.Invoke(this, hubEvent);

        foreach (var subscriber in _eventSubscribers.Values)
        {
            if (!subscriber.Writer.TryWrite(hubEvent))
            {
                _logger.LogDebug("Dropping generic event for one subscriber due to backpressure.");
            }
        }
    }

    private static Uri BuildWebSocketUri(string musicAssistantUrl)
    {
        if (!Uri.TryCreate(musicAssistantUrl, UriKind.Absolute, out var baseUri))
        {
            throw new InvalidOperationException($"Invalid Music Assistant URL: {musicAssistantUrl}");
        }

        var builder = new UriBuilder(baseUri)
        {
            Scheme = string.Equals(baseUri.Scheme, "https", StringComparison.OrdinalIgnoreCase)
                ? "wss"
                : "ws"
        };

        builder.Path = string.IsNullOrWhiteSpace(baseUri.AbsolutePath) || baseUri.AbsolutePath == "/"
            ? "/ws"
            : $"{baseUri.AbsolutePath.TrimEnd('/')}/ws";

        return builder.Uri;
    }

    private static async Task<string?> ReceiveTextMessageAsync(ClientWebSocket socket, CancellationToken cancellationToken)
    {
        var segment = new ArraySegment<byte>(new byte[8 * 1024]);
        using var messageBuffer = new MemoryStream();

        while (true)
        {
            var result = await socket.ReceiveAsync(segment, cancellationToken);

            if (result.MessageType == WebSocketMessageType.Close)
            {
                return null;
            }

            if (result.Count > 0)
            {
                await messageBuffer.WriteAsync(segment.Array!.AsMemory(segment.Offset, result.Count), cancellationToken);
            }

            if (result.EndOfMessage)
            {
                break;
            }
        }

        if (messageBuffer.Length == 0)
        {
            return null;
        }

        return Encoding.UTF8.GetString(messageBuffer.ToArray());
    }

    private static async Task CloseSocketAsync(ClientWebSocket socket, CancellationToken cancellationToken)
    {
        try
        {
            if (socket.State == WebSocketState.Open || socket.State == WebSocketState.CloseReceived)
            {
                await socket.CloseAsync(WebSocketCloseStatus.NormalClosure, "shutdown", cancellationToken);
            }
        }
        catch
        {
            // Ignore close failures on shutdown.
        }
    }
    #endregion
}
