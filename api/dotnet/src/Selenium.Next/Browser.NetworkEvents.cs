using System;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;

namespace Selenium.Next;

public sealed class NetworkRequestEventArgs : EventArgs
{
    public string RequestId { get; }
    public string Url { get; }
    public string Method { get; }
    public int RedirectCount { get; }

    internal NetworkRequestEventArgs(NetworkBeforeRequest request)
    {
        RequestId = request.RequestId;
        Url = request.Url;
        Method = request.Method;
        RedirectCount = request.RedirectCount;
    }
}

public abstract partial class Browser
{
    private readonly object _networkEventLock = new();
    private EventHandler<NetworkRequestEventArgs>? _networkRequestHandlers;
    private NetworkEventSubscription? _networkEventSubscription;
    private CancellationTokenSource? _networkEventCancellation;
    private Task? _networkEventPump;
    private bool _networkEventsDisposed;

    public event EventHandler<NetworkRequestEventArgs>? NetworkRequest
    {
        add
        {
            if (value is null) return;
            lock (_networkEventLock)
            {
                if (_networkEventsDisposed) throw new ObjectDisposedException(nameof(Browser));
                if (_networkEventPump is null ||
                    _networkEventPump.IsCompleted ||
                    _networkEventCancellation?.IsCancellationRequested == true)
                {
                    StartNetworkEventPump();
                }
                _networkRequestHandlers += value;
            }
        }
        remove
        {
            if (value is null) return;
            lock (_networkEventLock)
            {
                _networkRequestHandlers -= value;
            }
        }
    }

    private void StartNetworkEventPump()
    {
        var subscription = SubscribeNetworkBeforeRequest();
        var cancellation = new CancellationTokenSource();
        _networkEventSubscription = subscription;
        _networkEventCancellation = cancellation;
        _networkEventPump = Task.Run(() => PumpNetworkEvents(subscription, cancellation));
    }

    private void PumpNetworkEvents(NetworkEventSubscription subscription, CancellationTokenSource cancellation)
    {
        try
        {
            while (!cancellation.IsCancellationRequested)
            {
                NetworkBeforeRequest request;
                try
                {
                    request = subscription.Next(int.MaxValue);
                }
                catch (SeleniumException error) when (error.Kind == SeleniumErrorKind.Timeout)
                {
                    continue;
                }
                catch (Exception error) when (!cancellation.IsCancellationRequested)
                {
                    Trace.TraceError("Network request event delivery failed: {0}", error);
                    return;
                }
                catch (Exception) when (cancellation.IsCancellationRequested)
                {
                    return;
                }

                using (request)
                {
                    EventHandler<NetworkRequestEventArgs>? handlers;
                    lock (_networkEventLock)
                    {
                        if (cancellation.IsCancellationRequested) return;
                        handlers = _networkRequestHandlers;
                    }
                    if (handlers is null) continue;

                    var eventArgs = new NetworkRequestEventArgs(request);
                    foreach (EventHandler<NetworkRequestEventArgs> handler in handlers.GetInvocationList())
                    {
                        try
                        {
                            handler(this, eventArgs);
                        }
                        catch (Exception error)
                        {
                            Trace.TraceError("Network request event delivery failed: {0}", error);
                        }
                    }
                }
            }
        }
        finally
        {
            lock (_networkEventLock)
            {
                if (ReferenceEquals(_networkEventSubscription, subscription))
                {
                    _networkEventSubscription = null;
                    _networkEventCancellation = null;
                    _networkEventPump = null;
                }
            }
            subscription.Dispose();
            cancellation.Dispose();
        }
    }

    private void DisposeNetworkEvents()
    {
        lock (_networkEventLock)
        {
            _networkEventsDisposed = true;
            _networkRequestHandlers = null;
            _networkEventCancellation?.Cancel();
        }
    }
}
