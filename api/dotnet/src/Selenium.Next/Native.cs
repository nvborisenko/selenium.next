using System;
using System.Runtime.InteropServices;

namespace Selenium.Next;

internal static partial class Native
{
    public const string Lib = "selenium_next_core";

    [DllImport(Lib, EntryPoint = "se_abi_version")]
    private static extern uint se_abi_version();

    [DllImport(Lib, EntryPoint = "se_last_error")]
    private static extern IntPtr se_last_error();

    [DllImport(Lib, EntryPoint = "se_string_free")]
    private static extern void se_string_free(IntPtr s);

    [DllImport(Lib, EntryPoint = "se_array_free")]
    public static extern void se_array_free(IntPtr items, UIntPtr length);

    static Native()
    {
        NativeLoader.Init();
        var actual = se_abi_version();
        if (actual != ExpectedAbiVersion)
        {
            throw new InvalidOperationException(
                $"Native library ABI version {actual} does not match bindings ({ExpectedAbiVersion}).");
        }
    }

    /// <summary>Throws <see cref="SeleniumException"/> when a native call reports failure.</summary>
    public static void Check(int status)
    {
        if (status == 0) return;
        var message = Marshal.PtrToStringUTF8(se_last_error()) ?? "unknown native error";
        throw new SeleniumException((SeleniumErrorKind)status, message);
    }

    public static string TakeString(IntPtr ptr)
    {
        try { return Marshal.PtrToStringUTF8(ptr) ?? string.Empty; }
        finally { se_string_free(ptr); }
    }
}
