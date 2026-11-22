using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;

namespace Selenium.Next;

/// <summary>
/// Resolves the native core. Search order: SELENIUM_NEXT_NATIVE_DIR, the app directory,
/// then runtimes/&lt;rid&gt;/native (the NuGet layout).
/// </summary>
internal static class NativeLoader
{
    internal static void Init() => NativeLibrary.SetDllImportResolver(typeof(NativeLoader).Assembly, Resolve);

    private static IntPtr Resolve(string name, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (name != Native.Lib) return IntPtr.Zero;

        var file = OperatingSystem.IsWindows() ? $"{name}.dll"
            : OperatingSystem.IsMacOS() ? $"lib{name}.dylib"
            : $"lib{name}.so";
        var os = OperatingSystem.IsWindows() ? "win" : OperatingSystem.IsMacOS() ? "osx" : "linux";
        var arch = RuntimeInformation.ProcessArchitecture.ToString().ToLowerInvariant();

        var dirs = new[]
        {
            Environment.GetEnvironmentVariable("SELENIUM_NEXT_NATIVE_DIR"),
            AppContext.BaseDirectory,
            Path.Combine(AppContext.BaseDirectory, "runtimes", $"{os}-{arch}", "native"),
        };
        foreach (var dir in dirs)
        {
            if (string.IsNullOrEmpty(dir)) continue;
            var path = Path.Combine(dir, file);
            if (File.Exists(path) && NativeLibrary.TryLoad(path, out var handle)) return handle;
        }
        return IntPtr.Zero;
    }
}
