using System.Diagnostics;

namespace Selenium.Next.Tests;

public class ChromeTests
{
    [Test]
    public async Task GetStatus_ReturnsReadyStateAndMessage()
    {
        using var chrome = new Chrome(new ChromeOptions { Headless = true });

        var status = chrome.GetStatus();

        await Assert.That(status.Ready).IsFalse();
        await Assert.That(status.Message.Length > 0).IsTrue();
    }

    [Test]
    public async Task Navigate_EmitsNetworkRequestEvent()
    {
        using var chrome = new Chrome(new ChromeOptions { Headless = true });
        var requestReceived = new TaskCompletionSource<string>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        chrome.NetworkRequest += (_, request) =>
        {
            Console.WriteLine($"Network request: {request.Url}");
            if (request.Url.Contains("selenium.dev", StringComparison.OrdinalIgnoreCase))
            {
                requestReceived.TrySetResult(request.Url);
            }
        };
        chrome.Navigate("https://selenium.dev");
        var requestUrl = await requestReceived.Task.WaitAsync(TimeSpan.FromSeconds(60));

        using var page = chrome.DefaultPage;
        await Assert.That(page).IsNotNull();
        await Assert.That(requestUrl.Contains("selenium.dev", StringComparison.OrdinalIgnoreCase)).IsTrue();
    }

    [Test]
    public async Task NetworkRequestHandlerFailure_IsTracedAndOtherHandlersRun()
    {
        var reportedError = new TaskCompletionSource<string>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var requestReceived = new TaskCompletionSource<bool>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        using var listener = new ErrorTraceListener(reportedError);
        Trace.Listeners.Add(listener);
        try
        {
            using var chrome = new Chrome(new ChromeOptions { Headless = true });
            chrome.NetworkRequest += (_, request) =>
            {
                if (request.Url.Contains("selenium.dev", StringComparison.OrdinalIgnoreCase))
                {
                    throw new InvalidOperationException("request handler failed");
                }
            };
            chrome.NetworkRequest += (_, request) =>
            {
                if (request.Url.Contains("selenium.dev", StringComparison.OrdinalIgnoreCase))
                {
                    requestReceived.TrySetResult(true);
                }
            };

            chrome.Navigate("https://selenium.dev");
            var error = await reportedError.Task.WaitAsync(TimeSpan.FromSeconds(60));
            await requestReceived.Task.WaitAsync(TimeSpan.FromSeconds(60));
            await Assert.That(error.Contains("request handler failed", StringComparison.Ordinal)).IsTrue();
        }
        finally
        {
            Trace.Listeners.Remove(listener);
        }
    }

    private sealed class ErrorTraceListener(TaskCompletionSource<string> reportedError) : TraceListener
    {
        public override void Write(string? message) { }

        public override void WriteLine(string? message)
        {
            if (message?.Contains("request handler failed", StringComparison.Ordinal) == true)
            {
                reportedError.TrySetResult(message);
            }
        }
    }
}
