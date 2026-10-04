// Win32 / COM entry points used by ta-cs.
//
// Every signature is blittable (pointers and integers only), so the LibraryImport source
// generator emits a plain DllImport with no marshalling stub, and DisableRuntimeMarshalling
// guarantees the runtime never inserts one either. BOOL results are declared as int on purpose.

using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

[assembly: DisableRuntimeMarshalling]

namespace ToggleAudio.Bench;

internal static unsafe partial class Native
{
    // ---- HRESULTs -------------------------------------------------------------------------
    public const int S_OK = 0;
    /// <summary>CoInitializeEx: the thread already uses the other apartment model. Do not uninitialize.</summary>
    public const int RPC_E_CHANGED_MODE = unchecked((int)0x80010106);
    /// <summary>HRESULT_FROM_WIN32(ERROR_NOT_FOUND): unknown endpoint id, or no default device.</summary>
    public const int E_NOTFOUND = unchecked((int)0x80070490);
    /// <summary>Malformed endpoint id passed to IMMDeviceEnumerator::GetDevice.</summary>
    public const int E_INVALIDARG = unchecked((int)0x80070057);

    // ---- COM ------------------------------------------------------------------------------
    public const uint COINIT_APARTMENTTHREADED = 0x2;
    public const uint COINIT_DISABLE_OLE1DDE = 0x4;
    /// <summary>Both MMDevApi.dll and AudioSes.dll are in-process servers (ThreadingModel=Both).</summary>
    public const uint CLSCTX_INPROC_SERVER = 0x1;

    [LibraryImport("ole32.dll")]
    public static partial int CoInitializeEx(void* reserved, uint coInit);

    [LibraryImport("ole32.dll")]
    public static partial void CoUninitialize();

    [LibraryImport("ole32.dll")]
    public static partial int CoCreateInstance(Guid* clsid, void* outer, uint clsContext, Guid* iid, void** ppv);

    [LibraryImport("ole32.dll")]
    public static partial void CoTaskMemFree(void* pv);

    [LibraryImport("ole32.dll")]
    public static partial int PropVariantClear(PropVariant* pvar);

    // ---- Console / file output ------------------------------------------------------------
    public const int STD_OUTPUT_HANDLE = -11;
    public const int STD_ERROR_HANDLE = -12;

    [LibraryImport("kernel32.dll")]
    public static partial nint GetStdHandle(int stdHandle);

    [LibraryImport("kernel32.dll")]
    public static partial int GetConsoleMode(nint console, uint* mode);

    [LibraryImport("kernel32.dll")]
    public static partial int WriteConsoleW(nint console, char* buffer, uint charsToWrite, uint* charsWritten, void* reserved);

    [LibraryImport("kernel32.dll")]
    public static partial int WriteFile(nint file, byte* buffer, uint bytesToWrite, uint* bytesWritten, void* overlapped);

    // ---- Timing ---------------------------------------------------------------------------
    // These never block and never call back into managed code, so the GC transition
    // (a few dozen instructions per call) can be skipped.

    [LibraryImport("kernel32.dll"), SuppressGCTransition]
    public static partial int QueryPerformanceCounter(long* count);

    [LibraryImport("kernel32.dll"), SuppressGCTransition]
    public static partial int QueryPerformanceFrequency(long* frequency);

    [LibraryImport("kernel32.dll"), SuppressGCTransition]
    public static partial void GetSystemTimePreciseAsFileTime(long* fileTime);

    [LibraryImport("kernel32.dll")]
    public static partial int GetProcessTimes(nint process, long* creation, long* exit, long* kernel, long* user);

    /// <summary>Pseudo handle returned by GetCurrentProcess(); no call or CloseHandle needed.</summary>
    public const nint CurrentProcess = -1;
}

/// <summary>Win32 PROPVARIANT (16 bytes on x64). Only the VT_LPWSTR member is used.</summary>
[StructLayout(LayoutKind.Explicit, Size = 16)]
internal unsafe struct PropVariant
{
    public const ushort VT_LPWSTR = 31;

    [FieldOffset(0)] public ushort vt;
    [FieldOffset(8)] public char* pwszVal;
}

/// <summary>Win32 PROPERTYKEY (fmtid + pid).</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct PropertyKey
{
    public Guid fmtid;
    public uint pid;

    public PropertyKey(Guid fmtid, uint pid)
    {
        this.fmtid = fmtid;
        this.pid = pid;
    }
}
