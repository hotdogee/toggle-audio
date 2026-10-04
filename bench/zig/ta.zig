//! ta-zig: minimal "set the default Windows playback device" tool, native Zig.
//!
//! Part of the toggle-audio benchmark suite (bench/zig). It implements the
//! common bench CLI contract (docs/research/benchmark-method.md section 1):
//!
//!   ta-zig list                  one line per ACTIVE render endpoint: <id>\t<name>\t<flags>
//!   ta-zig get                   <id>\t<name> of the default (eConsole) render endpoint
//!   ta-zig set <id>              make <id> the default for eConsole, eMultimedia, eCommunications
//!   ta-zig toggle <idA> <idB>    if the default is idA set idB, otherwise set idA; prints the target id
//!   --timing (anywhere)          phase timestamps on stderr
//!
//! Exit codes: 0 OK, 1 usage, 2 COM failure, 3 device not found / not active, 4 no default device.
//!
//! Design notes
//! - No Zig std I/O, no allocator, no formatting library: Win32 and COM are
//!   declared by hand below (extern "kernel32" / extern "ole32" functions and
//!   extern-struct vtables), output goes through fixed static buffers. This keeps
//!   the binary small and independent from std API churn between Zig releases.
//! - COM objects are plain `extern struct { vtbl: *const Vtbl }` values; every
//!   method takes the object pointer as its first argument with the Windows x64
//!   calling convention (`callconv(.winapi)`). Unused vtable slots are declared
//!   as opaque pointers so the slot indexes stay exact.
//! - Every interface is released with `defer`, every CoTaskMem string is freed,
//!   every PROPVARIANT is cleared, and CoUninitialize runs last.
//! - COM runs in a single-threaded apartment (COINIT_APARTMENTTHREADED), like
//!   the product; both CLSIDs are ThreadingModel=Both, so STA costs nothing.
//! - Output is UTF-8 with LF line endings when stdout/stderr is redirected
//!   and UTF-16 via WriteConsoleW on a real console; never code-page dependent.
//! - `pub fn main() u8` with no parameters is the lightest std entry path: with
//!   -fsingle-threaded the start code just calls main and then
//!   RtlExitUserProcess (no allocator, no Io, no argv/environment parsing), so
//!   a hand-written wWinMainCRTStartup would gain nothing (verified: identical
//!   binary size).

const std = @import("std");

// ---------------------------------------------------------------------------
// Win32 base types and constants
// ---------------------------------------------------------------------------

const HRESULT = i32;
const BOOL = c_int;
const DWORD = u32;
const UINT = u32;
const HANDLE = ?*anyopaque;
const LPCWSTR = [*:0]const u16;
const LPWSTR = [*:0]u16;

const GUID = extern struct {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [8]u8,
};

const FILETIME = extern struct {
    low: u32,
    high: u32,

    fn toU64(self: FILETIME) u64 {
        return (@as(u64, self.high) << 32) | self.low;
    }
};

/// Reinterprets an HRESULT literal written as an unsigned hex constant.
fn hr(comptime value: u32) HRESULT {
    return @bitCast(value);
}

const S_OK: HRESULT = 0;
const S_FALSE: HRESULT = 1;
const E_POINTER = hr(0x80004003);
const E_INVALIDARG = hr(0x80070057);
/// HRESULT_FROM_WIN32(ERROR_NOT_FOUND): unknown endpoint id / no default device.
const E_NOTFOUND = hr(0x80070490);
/// The thread is already in another apartment; COM is usable but we must not uninitialize.
const RPC_E_CHANGED_MODE = hr(0x80010106);

fn failed(result: HRESULT) bool {
    return result < 0;
}

const STD_OUTPUT_HANDLE: DWORD = @bitCast(@as(i32, -11));
const STD_ERROR_HANDLE: DWORD = @bitCast(@as(i32, -12));

const COINIT_APARTMENTTHREADED: DWORD = 0x2;
const COINIT_DISABLE_OLE1DDE: DWORD = 0x4;
const CLSCTX_INPROC_SERVER: DWORD = 0x1;
const STGM_READ: DWORD = 0;
const DEVICE_STATE_ACTIVE: DWORD = 0x1;
const VT_LPWSTR: u16 = 31;

// ---------------------------------------------------------------------------
// kernel32 / ole32 imports
// ---------------------------------------------------------------------------

extern "kernel32" fn GetCommandLineW() callconv(.winapi) LPCWSTR;
extern "kernel32" fn GetStdHandle(nStdHandle: DWORD) callconv(.winapi) HANDLE;
extern "kernel32" fn GetConsoleMode(hConsole: HANDLE, lpMode: *DWORD) callconv(.winapi) BOOL;
extern "kernel32" fn WriteConsoleW(hConsole: HANDLE, buffer: [*]const u16, count: DWORD, written: ?*DWORD, reserved: ?*anyopaque) callconv(.winapi) BOOL;
extern "kernel32" fn WriteFile(hFile: HANDLE, buffer: [*]const u8, count: DWORD, written: ?*DWORD, overlapped: ?*anyopaque) callconv(.winapi) BOOL;
extern "kernel32" fn QueryPerformanceCounter(lpCount: *i64) callconv(.winapi) BOOL;
extern "kernel32" fn QueryPerformanceFrequency(lpFrequency: *i64) callconv(.winapi) BOOL;
extern "kernel32" fn GetCurrentProcess() callconv(.winapi) HANDLE;
extern "kernel32" fn GetProcessTimes(hProcess: HANDLE, creation: *FILETIME, exit: *FILETIME, kernel: *FILETIME, user: *FILETIME) callconv(.winapi) BOOL;
extern "kernel32" fn GetSystemTimePreciseAsFileTime(lpTime: *FILETIME) callconv(.winapi) void;

extern "ole32" fn CoInitializeEx(reserved: ?*anyopaque, coInit: DWORD) callconv(.winapi) HRESULT;
extern "ole32" fn CoUninitialize() callconv(.winapi) void;
extern "ole32" fn CoCreateInstance(clsid: *const GUID, outer: ?*anyopaque, clsContext: DWORD, iid: *const GUID, ppv: *?*anyopaque) callconv(.winapi) HRESULT;
extern "ole32" fn CoTaskMemFree(pv: ?*anyopaque) callconv(.winapi) void;
extern "ole32" fn PropVariantClear(pvar: *PROPVARIANT) callconv(.winapi) HRESULT;

// ---------------------------------------------------------------------------
// Core Audio and IPolicyConfig declarations
// ---------------------------------------------------------------------------

const CLSID_MMDeviceEnumerator = GUID{ .data1 = 0xBCDE0395, .data2 = 0xE52F, .data3 = 0x467C, .data4 = .{ 0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E } };
const IID_IMMDeviceEnumerator = GUID{ .data1 = 0xA95664D2, .data2 = 0x9614, .data3 = 0x4F35, .data4 = .{ 0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6 } };

/// CPolicyConfigClient (undocumented, AudioSes.dll) and IPolicyConfig (Windows 7 .. 11).
const CLSID_PolicyConfigClient = GUID{ .data1 = 0x870AF99C, .data2 = 0x171D, .data3 = 0x4F9E, .data4 = .{ 0xAF, 0x0D, 0xE6, 0x3D, 0xF4, 0x0C, 0x2B, 0xC9 } };
const IID_IPolicyConfig = GUID{ .data1 = 0xF8679F50, .data2 = 0x850A, .data3 = 0x41CF, .data4 = .{ 0x9C, 0x72, 0x43, 0x0F, 0x29, 0x02, 0x90, 0xC8 } };

/// Fallback: CPolicyConfigVistaClient and IPolicyConfigVista (SetDefaultEndpoint at slot 12).
const CLSID_PolicyConfigVistaClient = GUID{ .data1 = 0x294935CE, .data2 = 0xF637, .data3 = 0x4E7C, .data4 = .{ 0xA4, 0x1B, 0xAB, 0x25, 0x54, 0x60, 0xB8, 0x62 } };
const IID_IPolicyConfigVista = GUID{ .data1 = 0x568B9108, .data2 = 0x44BF, .data3 = 0x40B4, .data4 = .{ 0x90, 0x06, 0x86, 0xAF, 0xE5, 0xB5, 0xA6, 0x20 } };

const PROPERTYKEY = extern struct {
    fmtid: GUID,
    pid: DWORD,
};

/// PKEY_Device_FriendlyName {a45c254e-df1c-4efd-8020-67d146a850e0},14: the name Sound settings shows.
const PKEY_Device_FriendlyName = PROPERTYKEY{
    .fmtid = .{ .data1 = 0xA45C254E, .data2 = 0xDF1C, .data3 = 0x4EFD, .data4 = .{ 0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0 } },
    .pid = 14,
};

/// PROPVARIANT is 24 bytes on x64: a 2-byte VARTYPE, three reserved WORDs and a
/// 16-byte union (its largest members, e.g. BLOB, are a ULONG plus a pointer).
/// Only the VT_LPWSTR member is used here. A zero-initialized value equals
/// PropVariantInit().
const PROPVARIANT = extern struct {
    vt: u16 = 0,
    reserved1: u16 = 0,
    reserved2: u16 = 0,
    reserved3: u16 = 0,
    value: extern union {
        pwszVal: ?LPWSTR,
        raw: [2]u64,
    } = .{ .raw = .{ 0, 0 } },
};

comptime {
    std.debug.assert(@sizeOf(GUID) == 16);
    std.debug.assert(@sizeOf(PROPERTYKEY) == 20);
    std.debug.assert(@sizeOf(PROPVARIANT) == 24);
}

const EDataFlow = enum(c_int) { render = 0, capture = 1, all = 2 };
const ERole = enum(c_int) { console = 0, multimedia = 1, communications = 2 };

/// An unused vtable slot. Declared so that later slots keep their index.
const Slot = *const anyopaque;

/// IUnknown, the first three slots of every COM vtable. `self` is untyped
/// here so that `release` works for any interface.
const IUnknownVtbl = extern struct {
    QueryInterface: *const fn (self: *anyopaque, iid: *const GUID, out: *?*anyopaque) callconv(.winapi) HRESULT,
    AddRef: *const fn (self: *anyopaque) callconv(.winapi) u32,
    Release: *const fn (self: *anyopaque) callconv(.winapi) u32,
};

/// Calls IUnknown::Release on any interface pointer declared in this file.
fn release(object: anytype) void {
    _ = object.vtbl.unknown.Release(object);
}

const IMMDeviceEnumerator = extern struct {
    vtbl: *const Vtbl,
    const Vtbl = extern struct {
        unknown: IUnknownVtbl,
        EnumAudioEndpoints: *const fn (self: *IMMDeviceEnumerator, flow: EDataFlow, stateMask: DWORD, out: *?*IMMDeviceCollection) callconv(.winapi) HRESULT, // 3
        GetDefaultAudioEndpoint: *const fn (self: *IMMDeviceEnumerator, flow: EDataFlow, role: ERole, out: *?*IMMDevice) callconv(.winapi) HRESULT, // 4
        GetDevice: *const fn (self: *IMMDeviceEnumerator, id: LPCWSTR, out: *?*IMMDevice) callconv(.winapi) HRESULT, // 5
        RegisterEndpointNotificationCallback: Slot, // 6
        UnregisterEndpointNotificationCallback: Slot, // 7
    };
};

const IMMDeviceCollection = extern struct {
    vtbl: *const Vtbl,
    const Vtbl = extern struct {
        unknown: IUnknownVtbl,
        GetCount: *const fn (self: *IMMDeviceCollection, count: *UINT) callconv(.winapi) HRESULT, // 3
        Item: *const fn (self: *IMMDeviceCollection, index: UINT, out: *?*IMMDevice) callconv(.winapi) HRESULT, // 4
    };
};

const IMMDevice = extern struct {
    vtbl: *const Vtbl,
    const Vtbl = extern struct {
        unknown: IUnknownVtbl,
        Activate: Slot, // 3
        OpenPropertyStore: *const fn (self: *IMMDevice, access: DWORD, out: *?*IPropertyStore) callconv(.winapi) HRESULT, // 4
        GetId: *const fn (self: *IMMDevice, out: *?LPWSTR) callconv(.winapi) HRESULT, // 5
        GetState: *const fn (self: *IMMDevice, state: *DWORD) callconv(.winapi) HRESULT, // 6
    };
};

const IPropertyStore = extern struct {
    vtbl: *const Vtbl,
    const Vtbl = extern struct {
        unknown: IUnknownVtbl,
        GetCount: Slot, // 3
        GetAt: Slot, // 4
        GetValue: *const fn (self: *IPropertyStore, key: *const PROPERTYKEY, value: *PROPVARIANT) callconv(.winapi) HRESULT, // 5
        SetValue: Slot, // 6
        Commit: Slot, // 7
    };
};

const SetDefaultEndpointFn = *const fn (self: *anyopaque, id: LPCWSTR, role: ERole) callconv(.winapi) HRESULT;

/// IPolicyConfig on CPolicyConfigClient. Slot order verified against the
/// AudioSes.dll public PDB symbols (docs/research/core-audio-api.md B.2).
const IPolicyConfig = extern struct {
    vtbl: *const Vtbl,
    const Vtbl = extern struct {
        unknown: IUnknownVtbl,
        GetMixFormat: Slot, // 3
        GetDeviceFormat: Slot, // 4
        ResetDeviceFormat: Slot, // 5
        SetDeviceFormat: Slot, // 6
        GetProcessingPeriod: Slot, // 7
        SetProcessingPeriod: Slot, // 8
        GetShareMode: Slot, // 9
        SetShareMode: Slot, // 10
        GetPropertyValue: Slot, // 11
        SetPropertyValue: Slot, // 12
        SetDefaultEndpoint: SetDefaultEndpointFn, // 13
        SetEndpointVisibility: Slot, // 14
    };
};

/// IPolicyConfigVista on CPolicyConfigVistaClient: no ResetDeviceFormat, so
/// SetDefaultEndpoint sits at slot 12. (Several popular C# declarations get
/// this wrong; see core-audio-api.md B.2.)
const IPolicyConfigVista = extern struct {
    vtbl: *const Vtbl,
    const Vtbl = extern struct {
        unknown: IUnknownVtbl,
        GetMixFormat: Slot, // 3
        GetDeviceFormat: Slot, // 4
        SetDeviceFormat: Slot, // 5
        GetProcessingPeriod: Slot, // 6
        SetProcessingPeriod: Slot, // 7
        GetShareMode: Slot, // 8
        SetShareMode: Slot, // 9
        GetPropertyValue: Slot, // 10
        SetPropertyValue: Slot, // 11
        SetDefaultEndpoint: SetDefaultEndpointFn, // 12
        SetEndpointVisibility: Slot, // 13
    };
};

comptime {
    // Slot index = byte offset / pointer size.
    std.debug.assert(@offsetOf(IPolicyConfig.Vtbl, "SetDefaultEndpoint") == 13 * @sizeOf(usize));
    std.debug.assert(@offsetOf(IPolicyConfigVista.Vtbl, "SetDefaultEndpoint") == 12 * @sizeOf(usize));
    std.debug.assert(@offsetOf(IMMDevice.Vtbl, "GetState") == 6 * @sizeOf(usize));
    std.debug.assert(@offsetOf(IPropertyStore.Vtbl, "GetValue") == 5 * @sizeOf(usize));
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

const Error = error{
    /// A COM call failed; `failure` holds the step name and HRESULT.
    ComFailure,
    /// GetDevice did not know the id, or the endpoint is not ACTIVE.
    DeviceNotFound,
    /// GetDefaultAudioEndpoint returned E_NOTFOUND.
    NoDefaultDevice,
};

/// Details of the most recent COM failure, printed as "error: <step> hr=0x...".
var failure: struct { step: []const u8 = "", hr: HRESULT = 0 } = .{};

fn comFailure(step: []const u8, result: HRESULT) Error {
    failure = .{ .step = step, .hr = result };
    return error.ComFailure;
}

/// Turns a failed HRESULT into `error.ComFailure`, remembering which call failed.
fn check(result: HRESULT, step: []const u8) Error!void {
    if (failed(result)) return comFailure(step, result);
}

/// Checks an HRESULT and the interface pointer it was supposed to produce.
fn checkOut(comptime T: type, result: HRESULT, out: ?*T, step: []const u8) Error!*T {
    try check(result, step);
    return out orelse comFailure(step, E_POINTER);
}

// ---------------------------------------------------------------------------
// Output: UTF-16 text buffers flushed as UTF-8 (files/pipes) or UTF-16 (console)
// ---------------------------------------------------------------------------

/// Text is accumulated as UTF-16 (what COM returns) and written once at the end.
/// - Redirected handle (file, pipe, NUL): converted to UTF-8 and written with
///   WriteFile, so output never depends on the console code page (950 here).
/// - Real console: written with WriteConsoleW so CJK names display correctly.
///
/// The std handle id is a comptime parameter rather than a field so that the
/// global instances are entirely zero-initialized: their 32 KB arrays then
/// land in .bss and add nothing to the file size.
fn TextBuffer(comptime std_handle_id: DWORD) type {
    return struct {
        const Self = @This();
        const capacity = 16 * 1024;

        units: [capacity]u16 = @splat(0),
        len: usize = 0,

        fn appendUnit(self: *Self, unit: u16) void {
            if (self.len == capacity) self.flushKeepingSurrogate();
            self.units[self.len] = unit;
            self.len += 1;
        }

        fn appendWide(self: *Self, text: LPCWSTR) void {
            var i: usize = 0;
            while (text[i] != 0) : (i += 1) self.appendUnit(text[i]);
        }

        /// Like appendWide, but maps tab, CR and LF to a space so a value read
        /// from the system (an endpoint's friendly name) can never add a field
        /// or a line to the tab-separated output. Matches ta.c's tb_append_field.
        fn appendField(self: *Self, text: LPCWSTR) void {
            var i: usize = 0;
            while (text[i] != 0) : (i += 1) {
                const c = text[i];
                self.appendUnit(if (c == '\t' or c == '\r' or c == '\n') ' ' else c);
            }
        }

        fn appendAscii(self: *Self, text: []const u8) void {
            for (text) |c| self.appendUnit(c);
        }

        /// Appends "0x" and eight upper-case hex digits.
        fn appendHex32(self: *Self, value: u32) void {
            const digits = "0123456789ABCDEF";
            self.appendAscii("0x");
            var shift: u5 = 28;
            while (true) : (shift -= 4) {
                self.appendUnit(digits[(value >> shift) & 0xF]);
                if (shift == 0) break;
            }
        }

        fn appendDecimal(self: *Self, value: u64) void {
            var digits: [20]u8 = undefined;
            var n = value;
            var i: usize = digits.len;
            while (true) {
                i -= 1;
                digits[i] = @intCast('0' + n % 10);
                n /= 10;
                if (n == 0) break;
            }
            self.appendAscii(digits[i..]);
        }

        /// Appends a count of tenths as "<int>.<tenth>", e.g. 12345 -> "1234.5".
        fn appendTenths(self: *Self, tenths: i64) void {
            if (tenths < 0) self.appendUnit('-');
            const magnitude = @abs(tenths);
            self.appendDecimal(magnitude / 10);
            self.appendUnit('.');
            self.appendUnit(@intCast('0' + magnitude % 10));
        }

        /// Cold path (only reached with more than 16K UTF-16 units of output).
        /// Flushes everything except a trailing high surrogate, which is kept
        /// so a surrogate pair is never split across two writes.
        noinline fn flushKeepingSurrogate(self: *Self) void {
            const last = self.units[self.len - 1];
            if (last >= 0xD800 and last <= 0xDBFF) {
                self.len -= 1;
                self.flush();
                self.units[0] = last;
                self.len = 1;
            } else {
                self.flush();
            }
        }

        /// `noinline` keeps the encoder out of every append call site; ReleaseFast
        /// would otherwise inline it many times over for no measurable gain.
        noinline fn flush(self: *Self) void {
            if (self.len == 0) return;
            defer self.len = 0;
            const handle = GetStdHandle(std_handle_id);
            // INVALID_HANDLE_VALUE is -1; NULL means no handle was inherited.
            if (handle == null or @intFromPtr(handle) == std.math.maxInt(usize)) return;

            var mode: DWORD = 0;
            if (GetConsoleMode(handle, &mode) != 0) {
                var written: DWORD = 0;
                _ = WriteConsoleW(handle, &self.units, @intCast(self.len), &written, null);
                return;
            }
            writeUtf8(handle, self.units[0..self.len]);
        }
    };
}

/// Encodes UTF-16 to UTF-8 in chunks and writes it with WriteFile. Unpaired
/// surrogates become U+FFFD.
fn writeUtf8(handle: HANDLE, units: []const u16) void {
    const bytes = &utf8_scratch;
    var n: usize = 0;
    var i: usize = 0;
    while (i < units.len) {
        if (n > bytes.len - 4) {
            writeAll(handle, bytes[0..n]);
            n = 0;
        }
        var cp: u21 = units[i];
        i += 1;
        if (cp >= 0xD800 and cp <= 0xDBFF and i < units.len and units[i] >= 0xDC00 and units[i] <= 0xDFFF) {
            cp = 0x10000 + ((cp - 0xD800) << 10) + (units[i] - 0xDC00);
            i += 1;
        } else if (cp >= 0xD800 and cp <= 0xDFFF) {
            cp = 0xFFFD;
        }
        if (cp < 0x80) {
            bytes[n] = @intCast(cp);
            n += 1;
        } else if (cp < 0x800) {
            bytes[n] = @intCast(0xC0 | (cp >> 6));
            bytes[n + 1] = @intCast(0x80 | (cp & 0x3F));
            n += 2;
        } else if (cp < 0x10000) {
            bytes[n] = @intCast(0xE0 | (cp >> 12));
            bytes[n + 1] = @intCast(0x80 | ((cp >> 6) & 0x3F));
            bytes[n + 2] = @intCast(0x80 | (cp & 0x3F));
            n += 3;
        } else {
            bytes[n] = @intCast(0xF0 | (cp >> 18));
            bytes[n + 1] = @intCast(0x80 | ((cp >> 12) & 0x3F));
            bytes[n + 2] = @intCast(0x80 | ((cp >> 6) & 0x3F));
            bytes[n + 3] = @intCast(0x80 | (cp & 0x3F));
            n += 4;
        }
    }
    if (n > 0) writeAll(handle, bytes[0..n]);
}

/// WriteFile until every byte is written. lpNumberOfBytesWritten is required
/// for synchronous writes (it may be NULL only with an OVERLAPPED), and a pipe
/// may accept fewer bytes than requested. Gives up silently on a failed or
/// zero-length write: there is nowhere left to report an output error.
fn writeAll(handle: HANDLE, bytes: []const u8) void {
    var off: usize = 0;
    while (off < bytes.len) {
        var written: DWORD = 0;
        if (WriteFile(handle, bytes[off..].ptr, @intCast(bytes.len - off), &written, null) == 0 or written == 0) return;
        off += written;
    }
}

/// UTF-8 staging buffer for writeUtf8. Static (zero-initialized, so in .bss)
/// rather than on the stack: frames larger than a page would need a stack
/// probe (__chkstk) from compiler_rt.
var utf8_scratch: [4096]u8 = @splat(0);

var stdout: TextBuffer(STD_OUTPUT_HANDLE) = .{};
var stderr: TextBuffer(STD_ERROR_HANDLE) = .{};

// ---------------------------------------------------------------------------
// --timing: phase timestamps in microseconds since process creation
// ---------------------------------------------------------------------------

const Timing = struct {
    const max_phases = 8;

    enabled: bool = false,
    /// QueryPerformanceCounter at the first statement of main.
    entry_ticks: i64 = 0,
    /// Wall clock at the first statement of main (same FILETIME scale as the
    /// process creation time from GetProcessTimes).
    entry_wall: FILETIME = .{ .low = 0, .high = 0 },
    names: [max_phases][]const u8 = undefined,
    ticks: [max_phases]i64 = undefined,
    count: usize = 0,

    /// Two cheap clock reads, run unconditionally as the first statement of
    /// main, before argument parsing even knows whether --timing was given.
    /// Everything else (frequency, creation time) is fetched in `report`.
    fn start(self: *Timing) void {
        _ = QueryPerformanceCounter(&self.entry_ticks);
        GetSystemTimePreciseAsFileTime(&self.entry_wall);
        self.names[0] = "entry";
        self.ticks[0] = self.entry_ticks;
        self.count = 1;
    }

    fn mark(self: *Timing, name: []const u8) void {
        if (!self.enabled or self.count == max_phases) return;
        _ = QueryPerformanceCounter(&self.ticks[self.count]);
        self.names[self.count] = name;
        self.count += 1;
    }

    /// Queues "phase\t<name>\t<us>" lines on stderr. Times are microseconds
    /// since process creation with one decimal: the creation -> entry gap on
    /// the wall clock (GetProcessTimes vs. GetSystemTimePreciseAsFileTime),
    /// plus the QueryPerformanceCounter delta since entry.
    fn report(self: *Timing) void {
        if (!self.enabled) return;
        var ticks_per_second: i64 = 1;
        _ = QueryPerformanceFrequency(&ticks_per_second);
        var create_to_entry_100ns: i64 = 0;
        var creation: FILETIME = undefined;
        var exit_time: FILETIME = undefined;
        var kernel: FILETIME = undefined;
        var user: FILETIME = undefined;
        if (GetProcessTimes(GetCurrentProcess(), &creation, &exit_time, &kernel, &user) != 0) {
            create_to_entry_100ns = @as(i64, @bitCast(self.entry_wall.toU64())) - @as(i64, @bitCast(creation.toU64()));
        }
        for (self.names[0..self.count], self.ticks[0..self.count]) |name, ticks| {
            const delta_100ns = @divTrunc((ticks - self.entry_ticks) * 10_000_000, ticks_per_second);
            stderr.appendAscii("phase\t");
            stderr.appendAscii(name);
            stderr.appendUnit('\t');
            // 100 ns units are exactly tenths of a microsecond.
            stderr.appendTenths(create_to_entry_100ns + delta_100ns);
            stderr.appendUnit('\n');
        }
    }
};

var timing: Timing = .{};

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

/// Arguments after the program name, without "--timing", NUL-terminated
/// copies so they can be passed straight to COM.
const Args = struct {
    const max_args = 8;

    storage: [4096]u16 = @splat(0),
    used: usize = 0,
    items: [max_args]?LPCWSTR = @splat(null),
    count: usize = 0,
    overflow: bool = false,

    /// Splits GetCommandLineW() on spaces and tabs; double quotes group
    /// characters and are removed. Deliberately simpler than the MSVC CRT
    /// rules (no backslash escapes, no doubled quotes): they are not needed for this
    /// program's arguments (endpoint ids never contain quotes or backslashes),
    /// so CommandLineToArgvW and its shell32.dll load are avoided.
    fn parse(self: *Args, command_line: LPCWSTR) void {
        var i: usize = 0;
        // Program name: up to the closing quote, or up to the first blank.
        if (command_line[0] == '"') {
            i = 1;
            while (command_line[i] != 0 and command_line[i] != '"') i += 1;
            if (command_line[i] == '"') i += 1;
        } else {
            while (command_line[i] != 0 and !isBlank(command_line[i])) i += 1;
        }

        while (true) {
            while (isBlank(command_line[i])) i += 1;
            if (command_line[i] == 0) break;
            // No room left even for an empty argument's terminator: stop and
            // report a usage error rather than write past the buffer.
            if (self.used >= self.storage.len) {
                self.overflow = true;
                break;
            }
            const begin = self.used;
            var quoted = false;
            while (command_line[i] != 0 and (quoted or !isBlank(command_line[i]))) : (i += 1) {
                if (command_line[i] == '"') {
                    quoted = !quoted;
                } else if (self.used < self.storage.len - 1) {
                    self.storage[self.used] = command_line[i];
                    self.used += 1;
                } else {
                    self.overflow = true;
                }
            }
            // Safe: used < storage.len on entry and the copy loop stops one
            // unit short of the end, so this index is always in bounds.
            self.storage[self.used] = 0;
            self.used += 1;
            const arg: LPCWSTR = @ptrCast(&self.storage[begin]);
            if (equalsAscii(arg, "--timing")) {
                timing.enabled = true;
            } else if (self.count < max_args) {
                self.items[self.count] = arg;
                self.count += 1;
            } else {
                self.overflow = true;
            }
        }
    }

    fn isBlank(c: u16) bool {
        return c == ' ' or c == '\t';
    }
};

/// Static for the same reason as utf8_scratch: keeps main's frame small.
var command_line_args: Args = .{};

fn equalsAscii(a: LPCWSTR, b: []const u8) bool {
    for (b, 0..) |c, i| {
        if (a[i] != c) return false;
    }
    return a[b.len] == 0;
}

fn toLowerAscii(c: u16) u16 {
    return if (c >= 'A' and c <= 'Z') c + ('a' - 'A') else c;
}

/// Endpoint ids contain hex GUIDs; compare them ASCII case-insensitively so
/// ids typed in upper case still match what GetId returns (lower case).
fn idsEqual(a: LPCWSTR, b: LPCWSTR) bool {
    var i: usize = 0;
    while (true) : (i += 1) {
        if (toLowerAscii(a[i]) != toLowerAscii(b[i])) return false;
        if (a[i] == 0) return true;
    }
}

// ---------------------------------------------------------------------------
// Core Audio helpers
// ---------------------------------------------------------------------------

/// Endpoint id string owned by COM (CoTaskMemAlloc); free with `deinit`.
const OwnedId = struct {
    ptr: LPWSTR,

    fn deinit(self: OwnedId) void {
        CoTaskMemFree(self.ptr);
    }
};

fn getId(device: *IMMDevice) Error!OwnedId {
    var id: ?LPWSTR = null;
    try check(device.vtbl.GetId(device, &id), "GetId");
    return .{ .ptr = id orelse return comFailure("GetId", E_POINTER) };
}

/// Id of the default render endpoint for `role`, or null if there is none.
fn defaultId(enumerator: *IMMDeviceEnumerator, role: ERole) Error!?OwnedId {
    var out: ?*IMMDevice = null;
    const result = enumerator.vtbl.GetDefaultAudioEndpoint(enumerator, .render, role, &out);
    if (result == E_NOTFOUND) return null;
    const step = switch (role) {
        .console => "GetDefaultAudioEndpoint(eConsole)",
        .multimedia => "GetDefaultAudioEndpoint(eMultimedia)",
        .communications => "GetDefaultAudioEndpoint(eCommunications)",
    };
    const device = try checkOut(IMMDevice, result, out, step);
    defer release(device);
    return try getId(device);
}

/// Appends PKEY_Device_FriendlyName; appends nothing if the property is empty.
fn appendFriendlyName(device: *IMMDevice, text: anytype) Error!void {
    var out: ?*IPropertyStore = null;
    const store = try checkOut(IPropertyStore, device.vtbl.OpenPropertyStore(device, STGM_READ, &out), out, "FriendlyName");
    defer release(store);

    var value: PROPVARIANT = .{}; // PropVariantInit
    try check(store.vtbl.GetValue(store, &PKEY_Device_FriendlyName, &value), "FriendlyName");
    defer _ = PropVariantClear(&value);

    if (value.vt == VT_LPWSTR) {
        if (value.value.pwszVal) |name| text.appendField(name);
    }
}

/// Resolves `id` with GetDevice, requires DEVICE_STATE_ACTIVE and returns the
/// canonical id from GetId (also accepts a 24H2+ StableId as input).
fn resolveActive(enumerator: *IMMDeviceEnumerator, id: LPCWSTR) Error!OwnedId {
    var out: ?*IMMDevice = null;
    const result = enumerator.vtbl.GetDevice(enumerator, id, &out);
    if (result == E_NOTFOUND or result == E_INVALIDARG) {
        stderr.appendAscii("error: device not found: ");
        stderr.appendWide(id);
        stderr.appendAscii(" hr=");
        stderr.appendHex32(@bitCast(result));
        stderr.appendUnit('\n');
        return error.DeviceNotFound;
    }
    const device = try checkOut(IMMDevice, result, out, "GetDevice");
    defer release(device);

    // GetDevice also succeeds for NOTPRESENT, DISABLED and UNPLUGGED endpoints.
    var state: DWORD = 0;
    try check(device.vtbl.GetState(device, &state), "GetState");
    if (state != DEVICE_STATE_ACTIVE) {
        stderr.appendAscii("error: device not active: ");
        stderr.appendWide(id);
        stderr.appendAscii(" state=");
        stderr.appendHex32(state);
        stderr.appendUnit('\n');
        return error.DeviceNotFound;
    }
    return getId(device);
}

/// The undocumented policy-config object: IPolicyConfig (Windows 7 .. 11),
/// falling back to IPolicyConfigVista. Only SetDefaultEndpoint is used, so the
/// two are reduced to "object pointer + SetDefaultEndpoint function pointer".
const PolicyConfig = struct {
    object: *anyopaque,
    setDefaultEndpointFn: SetDefaultEndpointFn,

    fn create() Error!PolicyConfig {
        var out: ?*anyopaque = null;
        const result = CoCreateInstance(&CLSID_PolicyConfigClient, null, CLSCTX_INPROC_SERVER, &IID_IPolicyConfig, &out);
        if (!failed(result)) {
            if (out) |object| {
                const pc: *IPolicyConfig = @ptrCast(@alignCast(object));
                return .{ .object = object, .setDefaultEndpointFn = pc.vtbl.SetDefaultEndpoint };
            }
        }
        // Fallback for systems where IPolicyConfig is missing.
        out = null;
        const vista_result = CoCreateInstance(&CLSID_PolicyConfigVistaClient, null, CLSCTX_INPROC_SERVER, &IID_IPolicyConfigVista, &out);
        if (failed(vista_result) or out == null) {
            // Report the primary failure; it is the one that matters on Windows 7+.
            return comFailure("CoCreateInstance(PolicyConfigClient)", if (failed(result)) result else E_POINTER);
        }
        const vista: *IPolicyConfigVista = @ptrCast(@alignCast(out.?));
        return .{ .object = out.?, .setDefaultEndpointFn = vista.vtbl.SetDefaultEndpoint };
    }

    fn deinit(self: PolicyConfig) void {
        const unknown: *const *const IUnknownVtbl = @ptrCast(@alignCast(self.object));
        _ = unknown.*.Release(self.object);
    }

    fn setDefaultEndpoint(self: PolicyConfig, id: LPCWSTR, role: ERole) Error!void {
        try check(self.setDefaultEndpointFn(self.object, id, role), switch (role) {
            .console => "SetDefaultEndpoint(eConsole)",
            .multimedia => "SetDefaultEndpoint(eMultimedia)",
            .communications => "SetDefaultEndpoint(eCommunications)",
        });
    }
};

/// Validates `id` and makes it the default for all three roles, in the order
/// console, multimedia, communications. Runs even if `id` already is the
/// default (the bench "set-noop" scenario measures exactly that).
fn setDefaultAllRoles(enumerator: *IMMDeviceEnumerator, id: LPCWSTR) Error!OwnedId {
    const canonical = try resolveActive(enumerator, id);
    errdefer canonical.deinit();
    const policy = try PolicyConfig.create();
    defer policy.deinit();
    for ([_]ERole{ .console, .multimedia, .communications }) |role| {
        try policy.setDefaultEndpoint(canonical.ptr, role);
    }
    return canonical;
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

const Command = union(enum) {
    list,
    get,
    set: LPCWSTR,
    toggle: struct { a: LPCWSTR, b: LPCWSTR },
};

fn parseCommand(args: *const Args) ?Command {
    if (args.overflow or args.count == 0) return null;
    const name = args.items[0].?;
    const operands = args.count - 1;
    if (equalsAscii(name, "list") and operands == 0) return .list;
    if (equalsAscii(name, "get") and operands == 0) return .get;
    if (equalsAscii(name, "set") and operands == 1) return .{ .set = args.items[1].? };
    if (equalsAscii(name, "toggle") and operands == 2) return .{ .toggle = .{ .a = args.items[1].?, .b = args.items[2].? } };
    return null;
}

/// `list`: <id>\t<name>\t<flags>, flags = "*" default, "c" communications, "*c" both, "-" neither.
fn cmdList(enumerator: *IMMDeviceEnumerator) Error!void {
    const default_console = try defaultId(enumerator, .console);
    defer if (default_console) |id| id.deinit();
    const default_comms = try defaultId(enumerator, .communications);
    defer if (default_comms) |id| id.deinit();

    var out: ?*IMMDeviceCollection = null;
    const collection = try checkOut(IMMDeviceCollection, enumerator.vtbl.EnumAudioEndpoints(enumerator, .render, DEVICE_STATE_ACTIVE, &out), out, "EnumAudioEndpoints");
    defer release(collection);

    var count: UINT = 0;
    try check(collection.vtbl.GetCount(collection, &count), "GetCount");

    var index: UINT = 0;
    while (index < count) : (index += 1) {
        var item: ?*IMMDevice = null;
        const device = try checkOut(IMMDevice, collection.vtbl.Item(collection, index, &item), item, "Item");
        defer release(device);
        const id = try getId(device);
        defer id.deinit();

        stdout.appendWide(id.ptr);
        stdout.appendUnit('\t');
        try appendFriendlyName(device, &stdout);
        stdout.appendUnit('\t');
        const is_console = if (default_console) |d| idsEqual(d.ptr, id.ptr) else false;
        const is_comms = if (default_comms) |d| idsEqual(d.ptr, id.ptr) else false;
        if (is_console) stdout.appendUnit('*');
        if (is_comms) stdout.appendUnit('c');
        if (!is_console and !is_comms) stdout.appendUnit('-');
        stdout.appendUnit('\n');
    }
}

/// `get`: <id>\t<name> of the eConsole default.
fn cmdGet(enumerator: *IMMDeviceEnumerator) Error!void {
    var out: ?*IMMDevice = null;
    const result = enumerator.vtbl.GetDefaultAudioEndpoint(enumerator, .render, .console, &out);
    if (result == E_NOTFOUND) return error.NoDefaultDevice;
    const device = try checkOut(IMMDevice, result, out, "GetDefaultAudioEndpoint(eConsole)");
    defer release(device);
    const id = try getId(device);
    defer id.deinit();

    stdout.appendWide(id.ptr);
    stdout.appendUnit('\t');
    try appendFriendlyName(device, &stdout);
    stdout.appendUnit('\n');
}

/// `set <id>`: no output on success.
fn cmdSet(enumerator: *IMMDeviceEnumerator, id: LPCWSTR) Error!void {
    const canonical = try setDefaultAllRoles(enumerator, id);
    canonical.deinit();
}

/// `toggle <idA> <idB>`: the product's hot path. Prints the id it switched to.
fn cmdToggle(enumerator: *IMMDeviceEnumerator, a: LPCWSTR, b: LPCWSTR) Error!void {
    const current = try defaultId(enumerator, .console);
    const on_a = if (current) |id| idsEqual(id.ptr, a) else false;
    if (current) |id| id.deinit();

    const target = try setDefaultAllRoles(enumerator, if (on_a) b else a);
    defer target.deinit();
    stdout.appendWide(target.ptr);
    stdout.appendUnit('\n');
}

/// COM setup, the command itself, and cleanup in reverse order.
fn run(command: Command) Error!void {
    const init_result = CoInitializeEx(null, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    // S_OK and S_FALSE must be balanced by CoUninitialize; RPC_E_CHANGED_MODE
    // means COM is already usable on this thread in the other apartment.
    if (failed(init_result) and init_result != RPC_E_CHANGED_MODE) return comFailure("CoInitializeEx", init_result);
    defer if (!failed(init_result)) CoUninitialize();
    timing.mark("com_init");

    var out: ?*anyopaque = null;
    const raw = try checkOut(anyopaque, CoCreateInstance(&CLSID_MMDeviceEnumerator, null, CLSCTX_INPROC_SERVER, &IID_IMMDeviceEnumerator, &out), out, "CoCreateInstance(MMDeviceEnumerator)");
    const enumerator: *IMMDeviceEnumerator = @ptrCast(@alignCast(raw));
    defer release(enumerator);
    timing.mark("enumerator");

    switch (command) {
        .list => try cmdList(enumerator),
        .get => try cmdGet(enumerator),
        .set => |id| try cmdSet(enumerator, id),
        .toggle => |ids| try cmdToggle(enumerator, ids.a, ids.b),
    }
    timing.mark("work_done");
}

pub fn main() u8 {
    timing.start();

    command_line_args.parse(GetCommandLineW());

    const exit_code: u8 = if (parseCommand(&command_line_args)) |command| code: {
        run(command) catch |err| switch (err) {
            error.ComFailure => {
                stderr.appendAscii("error: ");
                stderr.appendAscii(failure.step);
                stderr.appendAscii(" hr=");
                stderr.appendHex32(@bitCast(failure.hr));
                stderr.appendUnit('\n');
                break :code 2;
            },
            error.DeviceNotFound => break :code 3, // message already queued
            error.NoDefaultDevice => {
                stderr.appendAscii("error: no default playback device\n");
                break :code 4;
            },
        };
        break :code 0;
    } else code: {
        // No COM work at all on this path: it measures the runtime floor.
        stderr.appendAscii("usage: ta-zig (list | get | set <id> | toggle <idA> <idB>) [--timing]\n");
        break :code 1;
    };

    stdout.flush();
    timing.mark("exit");
    timing.report();
    stderr.flush();
    return exit_code;
}
