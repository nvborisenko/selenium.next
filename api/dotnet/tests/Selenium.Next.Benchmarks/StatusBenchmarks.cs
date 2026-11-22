using BenchmarkDotNet.Attributes;
using OpenQA.Selenium.BiDi;
using OpenQA.Selenium.Chrome;
using Selenium.Next;
using SeleniumChromeDriver = OpenQA.Selenium.Chrome.ChromeDriver;
using SeleniumChromeOptions = OpenQA.Selenium.Chrome.ChromeOptions;

[MemoryDiagnoser]
public class StatusBenchmarks
{
    private Browser _seleniumNext = null!;
    private SeleniumChromeDriver _seleniumWebDriver = null!;
    private IBiDi _seleniumBiDi = null!;

    [GlobalSetup]
    public void Setup()
    {
        var driverPath = Environment.GetEnvironmentVariable("CHROMEDRIVER_PATH");
        var binaryPath = Environment.GetEnvironmentVariable("CHROME_BINARY_PATH");
        var nextOptions = new Selenium.Next.ChromeOptions
        {
            Headless = true,
            DriverPath = driverPath,
            BinaryPath = binaryPath,
        };
        var seleniumOptions = new SeleniumChromeOptions();
        seleniumOptions.UseWebSocketUrl = true;
        seleniumOptions.AddArgument("--headless=new");
        if (!string.IsNullOrWhiteSpace(binaryPath))
        {
            seleniumOptions.BinaryLocation = binaryPath;
        }

        _seleniumNext = new Selenium.Next.Chrome(nextOptions);

        try
        {
            if (string.IsNullOrWhiteSpace(driverPath))
            {
                _seleniumWebDriver = new SeleniumChromeDriver(seleniumOptions);
            }
            else
            {
                var fullDriverPath = Path.GetFullPath(driverPath);
                var service = ChromeDriverService.CreateDefaultService(
                    Path.GetDirectoryName(fullDriverPath)!,
                    Path.GetFileName(fullDriverPath));
                _seleniumWebDriver = new SeleniumChromeDriver(service, seleniumOptions);
            }

            _seleniumBiDi = _seleniumWebDriver.AsBiDiAsync().GetAwaiter().GetResult();
        }
        catch
        {
            _seleniumWebDriver?.Quit();
            _seleniumNext.Dispose();
            throw;
        }
    }

    [Benchmark(Baseline = true)]
    public Task<OpenQA.Selenium.BiDi.Session.StatusResult> Selenium_Status() =>
        _seleniumBiDi.StatusAsync();

    [Benchmark]
    public BrowserStatus SeleniumNext_Status() => _seleniumNext.GetStatus();

    [GlobalCleanup]
    public void Cleanup()
    {
        try
        {
            _seleniumBiDi?.DisposeAsync().AsTask().GetAwaiter().GetResult();
            _seleniumWebDriver?.Quit();
        }
        finally
        {
            _seleniumNext?.Dispose();
        }
    }
}
