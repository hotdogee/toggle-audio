/*
 * ta.c - toggle-audio benchmark reference implementation in plain C (MSVC).
 *
 * Implements the common bench CLI contract (docs/research/benchmark-method.md
 * section 1, with the console-subsystem deviation from docs/DESIGN.md section 12
 * and the output-format deviations listed in README.md):
 *
 *   ta-c list                 one line per ACTIVE render endpoint: "<id>\t<name>\t<flags>"
 *                             flags: "*" default (eConsole), "c" default communications,
 *                             "*c" both, "-" neither
 *   ta-c get                  "<id>\t<name>" of the default (eConsole) render endpoint
 *   ta-c set <id>             validate (exists + ACTIVE), then SetDefaultEndpoint for
 *                             eConsole, eMultimedia, eCommunications (always, even if
 *                             the endpoint already is the default)
 *   ta-c toggle <idA> <idB>   if the eConsole default is idA set idB, else set idA;
 *                             prints the target id
 *   ta-c                      usage on stderr, exit 1, no COM work (runtime floor)
 *   --timing (anywhere)       phase stamps on stderr: "phase\t<name>\t<us since process creation>"
 *
 * Exit codes: 0 OK, 1 usage, 2 COM/Win32 failure (HRESULT printed), 3 device not
 * found or not active, 4 no default device.
 *
 * Output: redirected handles (pipe/file/NUL) get UTF-8 bytes with "\n" line
 * endings via WriteFile; a real console gets UTF-16 via WriteConsoleW so CJK
 * names render regardless of the console code page. Nothing goes through the
 * CRT's code-page-dependent printf family.
 *
 * The program uses no CRT functions at all, so the same source builds:
 *   - with the CRT (/MT or /MD): entry point is wmain, argv comes from the CRT;
 *   - without the CRT (/DTA_NOCRT, /NODEFAULTLIB /ENTRY:entry): the entry point
 *     parses GetCommandLineW() itself and memset/memcpy are supplied below.
 *
 * Only kernel32 and ole32 are imported. CommandLineToArgvW (shell32) is avoided
 * on purpose: every extra DLL costs loader time, which is what this measures.
 *
 * IPolicyConfig is an undocumented interface; its layout here was verified
 * against Microsoft public symbols for AudioSes.dll (docs/research/core-audio-api.md
 * sections B.2 and D.2).
 */

#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#define COBJMACROS          /* C-friendly IFoo_Method(p, ...) wrappers for COM vtables */
#define WIN32_LEAN_AND_MEAN

#include <windows.h>
#include <initguid.h>       /* make DEFINE_GUID / DEFINE_PROPERTYKEY emit definitions in this TU */
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>  /* PKEY_Device_FriendlyName */

/* ------------------------------------------------------------------------- */
/* GUIDs                                                                     */
/* ------------------------------------------------------------------------- */

/* mmdeviceapi.h only *declares* these two for C, and no SDK import library
 * defines them, so they must be defined here (otherwise LNK2019). */
DEFINE_GUID(CLSID_MMDeviceEnumerator,
            0xbcde0395, 0xe52f, 0x467c, 0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e);
DEFINE_GUID(IID_IMMDeviceEnumerator,
            0xa95664d2, 0x9614, 0x4f35, 0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6);

/* Undocumented policy-config objects in AudioSes.dll (ThreadingModel=Both). */
DEFINE_GUID(CLSID_CPolicyConfigClient,
            0x870af99c, 0x171d, 0x4f9e, 0xaf, 0x0d, 0xe6, 0x3d, 0xf4, 0x0c, 0x2b, 0xc9);
DEFINE_GUID(IID_IPolicyConfig,
            0xf8679f50, 0x850a, 0x41cf, 0x9c, 0x72, 0x43, 0x0f, 0x29, 0x02, 0x90, 0xc8);
DEFINE_GUID(CLSID_CPolicyConfigVistaClient,
            0x294935ce, 0xf637, 0x4e7c, 0xa4, 0x1b, 0xab, 0x25, 0x54, 0x60, 0xb8, 0x62);
DEFINE_GUID(IID_IPolicyConfigVista,
            0x568b9108, 0x44bf, 0x40b4, 0x90, 0x06, 0x86, 0xaf, 0xe5, 0xb5, 0xa6, 0x20);

/* ------------------------------------------------------------------------- */
/* Hand-declared IPolicyConfig / IPolicyConfigVista vtables                  */
/* ------------------------------------------------------------------------- */

/* Methods we never call are declared as opaque pointer-sized placeholders:
 * only the slot index matters, and every slot is one pointer wide.
 * IPolicyConfig::SetDefaultEndpoint is slot 13 (0-based, IUnknown = 0..2). */
typedef struct IPolicyConfig IPolicyConfig;
typedef struct IPolicyConfigVtbl {
    HRESULT (STDMETHODCALLTYPE *QueryInterface)(IPolicyConfig *self, REFIID riid, void **ppv); /* 0 */
    ULONG   (STDMETHODCALLTYPE *AddRef)(IPolicyConfig *self);                                  /* 1 */
    ULONG   (STDMETHODCALLTYPE *Release)(IPolicyConfig *self);                                 /* 2 */
    void *GetMixFormat;                                                                        /* 3 */
    void *GetDeviceFormat;                                                                     /* 4 */
    void *ResetDeviceFormat;                                                                   /* 5 */
    void *SetDeviceFormat;                                                                     /* 6 */
    void *GetProcessingPeriod;                                                                 /* 7 */
    void *SetProcessingPeriod;                                                                 /* 8 */
    void *GetShareMode;                                                                        /* 9 */
    void *SetShareMode;                                                                        /* 10 */
    void *GetPropertyValue;                                                                    /* 11 */
    void *SetPropertyValue;                                                                    /* 12 */
    HRESULT (STDMETHODCALLTYPE *SetDefaultEndpoint)(IPolicyConfig *self, LPCWSTR device_id,
                                                    ERole role);                               /* 13 */
    void *SetEndpointVisibility;                                                               /* 14 */
} IPolicyConfigVtbl;
struct IPolicyConfig { const IPolicyConfigVtbl *lpVtbl; };

/* Vista-era interface, used only as a fallback. It has NO ResetDeviceFormat,
 * so SetDefaultEndpoint is slot 12 (SoundSwitch/AudioDeviceCmdlets get this wrong). */
typedef struct IPolicyConfigVista IPolicyConfigVista;
typedef struct IPolicyConfigVistaVtbl {
    HRESULT (STDMETHODCALLTYPE *QueryInterface)(IPolicyConfigVista *self, REFIID riid, void **ppv); /* 0 */
    ULONG   (STDMETHODCALLTYPE *AddRef)(IPolicyConfigVista *self);                                  /* 1 */
    ULONG   (STDMETHODCALLTYPE *Release)(IPolicyConfigVista *self);                                 /* 2 */
    void *GetMixFormat;                                                                             /* 3 */
    void *GetDeviceFormat;                                                                          /* 4 */
    void *SetDeviceFormat;                                                                          /* 5 */
    void *GetProcessingPeriod;                                                                      /* 6 */
    void *SetProcessingPeriod;                                                                      /* 7 */
    void *GetShareMode;                                                                             /* 8 */
    void *SetShareMode;                                                                             /* 9 */
    void *GetPropertyValue;                                                                         /* 10 */
    void *SetPropertyValue;                                                                         /* 11 */
    HRESULT (STDMETHODCALLTYPE *SetDefaultEndpoint)(IPolicyConfigVista *self, LPCWSTR device_id,
                                                    ERole role);                                    /* 12 */
    void *SetEndpointVisibility;                                                                    /* 13 */
} IPolicyConfigVistaVtbl;
struct IPolicyConfigVista { const IPolicyConfigVistaVtbl *lpVtbl; };

/* ------------------------------------------------------------------------- */
/* No-CRT support                                                            */
/* ------------------------------------------------------------------------- */

#ifdef TA_NOCRT
#include <intrin.h>
/* The compiler may emit calls to memset/memcpy (PropVariantInit is a memset
 * macro, and struct initialisers can become memset). Without the CRT we must
 * provide them. "#pragma function" tells the compiler that we define these
 * normally-intrinsic functions; the rep stosb / rep movsb intrinsics keep the
 * optimiser from turning the body back into a recursive memset call. */
#pragma function(memset, memcpy)
void *__cdecl memset(void *dst, int value, size_t count)
{
    __stosb((unsigned char *)dst, (unsigned char)value, count);
    return dst;
}
void *__cdecl memcpy(void *dst, const void *src, size_t count)
{
    __movsb((unsigned char *)dst, (const unsigned char *)src, count);
    return dst;
}
#endif

/* ------------------------------------------------------------------------- */
/* Exit codes and constants                                                  */
/* ------------------------------------------------------------------------- */

enum {
    EXIT_OK = 0,
    EXIT_USAGE = 1,
    EXIT_COM_FAILURE = 2,
    EXIT_DEVICE_NOT_FOUND = 3,
    EXIT_NO_DEFAULT = 4
};

#ifndef E_NOTFOUND
#define E_NOTFOUND HRESULT_FROM_WIN32(ERROR_NOT_FOUND) /* 0x80070490 */
#endif

static const WCHAR USAGE_TEXT[] =
    L"usage: <exe> (list | get | set <id> | toggle <idA> <idB>) [--timing]\n";

/* ------------------------------------------------------------------------- */
/* Growable UTF-16 text buffer (process heap, no CRT)                        */
/* ------------------------------------------------------------------------- */

typedef struct TextBuf {
    WCHAR *data;
    SIZE_T len;     /* characters used, excluding any terminator */
    SIZE_T cap;     /* characters allocated */
    BOOL oom;       /* an allocation failed; contents are truncated */
} TextBuf;

static BOOL tb_reserve(TextBuf *b, SIZE_T extra)
{
    SIZE_T need = b->len + extra;
    SIZE_T new_cap;
    WCHAR *p;

    if (b->oom) return FALSE;
    if (need <= b->cap) return TRUE;
    new_cap = b->cap ? b->cap * 2 : 256;
    while (new_cap < need) new_cap *= 2;
    p = b->data
        ? (WCHAR *)HeapReAlloc(GetProcessHeap(), 0, b->data, new_cap * sizeof(WCHAR))
        : (WCHAR *)HeapAlloc(GetProcessHeap(), 0, new_cap * sizeof(WCHAR));
    if (!p) { b->oom = TRUE; return FALSE; }
    b->data = p;
    b->cap = new_cap;
    return TRUE;
}

static void tb_append_n(TextBuf *b, const WCHAR *s, SIZE_T n)
{
    SIZE_T i;
    if (!tb_reserve(b, n)) return;
    for (i = 0; i < n; i++) b->data[b->len + i] = s[i];
    b->len += n;
}

static SIZE_T wstr_len(const WCHAR *s)
{
    SIZE_T n = 0;
    if (s) while (s[n]) n++;
    return n;
}

static void tb_append(TextBuf *b, const WCHAR *s)
{
    tb_append_n(b, s, wstr_len(s));
}

static void tb_append_char(TextBuf *b, WCHAR c)
{
    tb_append_n(b, &c, 1);
}

/* Appends a friendly name, replacing tab/CR/LF with a space so one endpoint
 * always stays one tab-separated line. */
static void tb_append_field(TextBuf *b, const WCHAR *s)
{
    SIZE_T i, n = wstr_len(s);
    if (!tb_reserve(b, n)) return;
    for (i = 0; i < n; i++) {
        WCHAR c = s[i];
        b->data[b->len + i] = (c == L'\t' || c == L'\r' || c == L'\n') ? L' ' : c;
    }
    b->len += n;
}

static void tb_append_u64(TextBuf *b, ULONGLONG v)
{
    WCHAR digits[20];
    int n = 0;
    do { digits[n++] = (WCHAR)(L'0' + (v % 10)); v /= 10; } while (v);
    while (n) tb_append_char(b, digits[--n]);
}

/* Appends a value given in tenths of a unit as "<int>.<tenth>", e.g. 1234 -> "123.4". */
static void tb_append_tenths(TextBuf *b, LONGLONG tenths)
{
    ULONGLONG mag;
    if (tenths < 0) { tb_append_char(b, L'-'); mag = (ULONGLONG)(-tenths); }
    else mag = (ULONGLONG)tenths;
    tb_append_u64(b, mag / 10);
    tb_append_char(b, L'.');
    tb_append_char(b, (WCHAR)(L'0' + (mag % 10)));
}

/* Appends "0x" followed by exactly 8 upper-case hex digits. */
static void tb_append_hex32(TextBuf *b, DWORD v)
{
    static const WCHAR hex[] = L"0123456789ABCDEF";
    int shift;
    tb_append(b, L"0x");
    for (shift = 28; shift >= 0; shift -= 4) tb_append_char(b, hex[(v >> shift) & 0xF]);
}

static void tb_free(TextBuf *b)
{
    if (b->data) HeapFree(GetProcessHeap(), 0, b->data);
    b->data = NULL;
    b->len = b->cap = 0;
}

/* ------------------------------------------------------------------------- */
/* Output to std handles                                                     */
/* ------------------------------------------------------------------------- */

/* Writes the whole buffer to a std handle in one go. A console gets UTF-16 via
 * WriteConsoleW; anything else (pipe, file, NUL) gets UTF-8 via WriteFile.
 * A missing handle (GUI-subsystem build launched without redirection) is
 * silently ignored, as is a write error: there is nowhere left to report it. */
static void write_std(DWORD which, const TextBuf *b)
{
    HANDLE h = GetStdHandle(which);
    DWORD mode, written;
    int bytes;
    char *utf8;

    if (b->len == 0 || h == NULL || h == INVALID_HANDLE_VALUE) return;

    if (GetConsoleMode(h, &mode)) {
        SIZE_T done = 0;
        while (done < b->len) {
            DWORD chunk = (DWORD)((b->len - done) > 16384 ? 16384 : (b->len - done));
            if (!WriteConsoleW(h, b->data + done, chunk, &written, NULL) || written == 0) return;
            done += written;
        }
        return;
    }

    bytes = WideCharToMultiByte(CP_UTF8, 0, b->data, (int)b->len, NULL, 0, NULL, NULL);
    if (bytes <= 0) return;
    utf8 = (char *)HeapAlloc(GetProcessHeap(), 0, (SIZE_T)bytes);
    if (!utf8) return;
    if (WideCharToMultiByte(CP_UTF8, 0, b->data, (int)b->len, utf8, bytes, NULL, NULL) == bytes) {
        DWORD done = 0;
        while (done < (DWORD)bytes) {
            if (!WriteFile(h, utf8 + done, (DWORD)bytes - done, &written, NULL) || written == 0) break;
            done += written;
        }
    }
    HeapFree(GetProcessHeap(), 0, utf8);
}

/* ------------------------------------------------------------------------- */
/* Timing (--timing)                                                         */
/* ------------------------------------------------------------------------- */

#define MAX_PHASES 8

typedef struct Timing {
    BOOL enabled;
    LARGE_INTEGER qpc_entry;      /* QueryPerformanceCounter at entry */
    FILETIME wall_entry;          /* GetSystemTimePreciseAsFileTime at entry */
    int count;
    const WCHAR *names[MAX_PHASES];
    LONGLONG ticks[MAX_PHASES];   /* raw QPC values; formatted only at the end */
} Timing;

static Timing g_timing;

/* First thing the entry point does: two cheap clock reads. */
static void timing_start(void)
{
    QueryPerformanceCounter(&g_timing.qpc_entry);
    GetSystemTimePreciseAsFileTime(&g_timing.wall_entry);
}

static void phase(const WCHAR *name)
{
    LARGE_INTEGER now;
    if (!g_timing.enabled || g_timing.count >= MAX_PHASES) return;
    QueryPerformanceCounter(&now);
    g_timing.names[g_timing.count] = name;
    g_timing.ticks[g_timing.count] = now.QuadPart;
    g_timing.count++;
}

static ULONGLONG filetime_u64(FILETIME ft)
{
    return ((ULONGLONG)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
}

/* Formats "phase\t<name>\t<us since process creation>\n" lines.
 * The process creation time comes from GetProcessTimes (100 ns units); the
 * creation -> entry gap is measured on the wall clock, everything after entry
 * on QPC. Values are in microseconds with one decimal (100 ns = 0.1 us). */
static void timing_format(TextBuf *out)
{
    FILETIME creation, exit_time, kernel, user;
    LONGLONG create_to_entry = 0; /* tenths of a microsecond */
    LARGE_INTEGER freq;
    int i;

    if (GetProcessTimes(GetCurrentProcess(), &creation, &exit_time, &kernel, &user))
        create_to_entry = (LONGLONG)(filetime_u64(g_timing.wall_entry) - filetime_u64(creation));
    QueryPerformanceFrequency(&freq);

    for (i = 0; i < g_timing.count; i++) {
        /* Split into whole seconds + remainder so the multiply cannot overflow. */
        ULONGLONG delta = (ULONGLONG)(g_timing.ticks[i] - g_timing.qpc_entry.QuadPart);
        ULONGLONG f = (ULONGLONG)freq.QuadPart;
        ULONGLONG tenths = (delta / f) * 10000000ULL + ((delta % f) * 10000000ULL) / f;
        tb_append(out, L"phase\t");
        tb_append(out, g_timing.names[i]);
        tb_append_char(out, L'\t');
        tb_append_tenths(out, create_to_entry + (LONGLONG)tenths);
        tb_append_char(out, L'\n');
    }
}

/* ------------------------------------------------------------------------- */
/* Error reporting                                                           */
/* ------------------------------------------------------------------------- */

typedef struct App {
    TextBuf out;   /* stdout, written once at the end */
    TextBuf err;   /* stderr, written once at the end (errors, then timing) */
} App;

/* "error: <step> hr=0x8007xxxx" and exit code 2. */
static int fail_hr(App *app, const WCHAR *step, HRESULT hr)
{
    tb_append(&app->err, L"error: ");
    tb_append(&app->err, step);
    tb_append(&app->err, L" hr=");
    tb_append_hex32(&app->err, (DWORD)hr);
    tb_append_char(&app->err, L'\n');
    return EXIT_COM_FAILURE;
}

static BOOL ids_equal(const WCHAR *a, const WCHAR *b)
{
    /* Endpoint ids are GUID-based, so compare ordinally and case-insensitively. */
    return a && b && CompareStringOrdinal(a, -1, b, -1, TRUE) == CSTR_EQUAL;
}

/* ------------------------------------------------------------------------- */
/* Core Audio helpers                                                        */
/* ------------------------------------------------------------------------- */

/* Appends the endpoint's PKEY_Device_FriendlyName (exactly what Sound settings
 * shows, e.g. "PG42UQ (NVIDIA High Definition Audio)"; on a zh-TW system the
 * localized part is CJK, e.g. U+5587 U+53ED "(FiiO BTA30 PRO)") to b. A missing name (VT_EMPTY) appends
 * nothing and is not an error. */
static HRESULT append_friendly_name(IMMDevice *device, TextBuf *b)
{
    IPropertyStore *store = NULL;
    PROPVARIANT value;
    HRESULT hr = IMMDevice_OpenPropertyStore(device, STGM_READ, &store);
    if (FAILED(hr)) return hr;

    PropVariantInit(&value);
    hr = IPropertyStore_GetValue(store, &PKEY_Device_FriendlyName, &value);
    if (SUCCEEDED(hr) && value.vt == VT_LPWSTR) tb_append_field(b, value.pwszVal);
    PropVariantClear(&value);
    IPropertyStore_Release(store);
    return SUCCEEDED(hr) ? S_OK : hr;
}

/* Id of the default render endpoint for a role. *id receives a CoTaskMem
 * string (caller frees) or NULL. Returns E_NOTFOUND if there is no default. */
static HRESULT get_default_id(IMMDeviceEnumerator *enumerator, ERole role, LPWSTR *id)
{
    IMMDevice *device = NULL;
    HRESULT hr;

    *id = NULL;
    hr = IMMDeviceEnumerator_GetDefaultAudioEndpoint(enumerator, eRender, role, &device);
    if (FAILED(hr)) return hr;
    hr = IMMDevice_GetId(device, id);
    IMMDevice_Release(device);
    return hr;
}

/* Resolves a user-supplied endpoint id and checks that it is ACTIVE.
 * GetDevice succeeds for NOTPRESENT/DISABLED/UNPLUGGED endpoints too, so the
 * state check is mandatory. On success *canonical_id receives the id as
 * reported by the device itself (CoTaskMem string, caller frees); that is the
 * string handed to SetDefaultEndpoint. Returns an exit code. */
static int resolve_active_endpoint(App *app, IMMDeviceEnumerator *enumerator,
                                   const WCHAR *id, LPWSTR *canonical_id)
{
    IMMDevice *device = NULL;
    DWORD state = 0;
    HRESULT hr;

    *canonical_id = NULL;
    hr = IMMDeviceEnumerator_GetDevice(enumerator, id, &device);
    if (hr == E_NOTFOUND || hr == E_INVALIDARG) {
        tb_append(&app->err, L"error: device not found: ");
        tb_append(&app->err, id);
        tb_append(&app->err, L" hr=");
        tb_append_hex32(&app->err, (DWORD)hr);
        tb_append_char(&app->err, L'\n');
        return EXIT_DEVICE_NOT_FOUND;
    }
    if (FAILED(hr)) return fail_hr(app, L"GetDevice", hr);

    hr = IMMDevice_GetState(device, &state);
    if (FAILED(hr)) {
        IMMDevice_Release(device);
        return fail_hr(app, L"GetState", hr);
    }
    if (state != DEVICE_STATE_ACTIVE) {
        IMMDevice_Release(device);
        tb_append(&app->err, L"error: device not active: ");
        tb_append(&app->err, id);
        tb_append(&app->err, L" state=");
        tb_append_hex32(&app->err, state);
        tb_append_char(&app->err, L'\n');
        return EXIT_DEVICE_NOT_FOUND;
    }

    hr = IMMDevice_GetId(device, canonical_id);
    IMMDevice_Release(device);
    if (FAILED(hr)) return fail_hr(app, L"GetId", hr);
    return EXIT_OK;
}

/* Makes the endpoint the default for eConsole, eMultimedia and eCommunications
 * (in that order) through IPolicyConfig, falling back to IPolicyConfigVista
 * if the modern interface is unavailable. Returns an exit code. */
static int set_default_all_roles(App *app, const WCHAR *id)
{
    static const ERole roles[3] = { eConsole, eMultimedia, eCommunications };
    IPolicyConfig *policy = NULL;
    IPolicyConfigVista *policy_vista = NULL;
    HRESULT hr, hr_vista;
    int i;

    hr = CoCreateInstance(&CLSID_CPolicyConfigClient, NULL, CLSCTX_INPROC_SERVER,
                          &IID_IPolicyConfig, (void **)&policy);
    if (FAILED(hr)) {
        hr_vista = CoCreateInstance(&CLSID_CPolicyConfigVistaClient, NULL, CLSCTX_INPROC_SERVER,
                                    &IID_IPolicyConfigVista, (void **)&policy_vista);
        if (FAILED(hr_vista)) return fail_hr(app, L"CoCreateInstance(PolicyConfigClient)", hr);
    }

    for (i = 0; i < 3; i++) {
        hr = policy ? policy->lpVtbl->SetDefaultEndpoint(policy, id, roles[i])
                    : policy_vista->lpVtbl->SetDefaultEndpoint(policy_vista, id, roles[i]);
        if (FAILED(hr)) break;
    }

    if (policy) policy->lpVtbl->Release(policy);
    if (policy_vista) policy_vista->lpVtbl->Release(policy_vista);
    if (FAILED(hr)) {
        static const WCHAR *const steps[3] = {
            L"SetDefaultEndpoint(eConsole)", L"SetDefaultEndpoint(eMultimedia)",
            L"SetDefaultEndpoint(eCommunications)"
        };
        /* The endpoint vanished after it was validated (unplugged mid-call):
         * report it as "device not found" (exit 3), not as a COM failure. */
        if (hr == E_NOTFOUND) {
            fail_hr(app, steps[i], hr);
            return EXIT_DEVICE_NOT_FOUND;
        }
        return fail_hr(app, steps[i], hr);
    }
    return EXIT_OK;
}

/* ------------------------------------------------------------------------- */
/* Commands                                                                  */
/* ------------------------------------------------------------------------- */

static int cmd_list(App *app, IMMDeviceEnumerator *enumerator)
{
    LPWSTR default_id = NULL, comms_id = NULL;
    IMMDeviceCollection *collection = NULL;
    UINT count = 0, i;
    int rc = EXIT_OK;
    HRESULT hr;

    /* No default device is not an error for list: it just means no marker. */
    hr = get_default_id(enumerator, eConsole, &default_id);
    if (FAILED(hr) && hr != E_NOTFOUND) { rc = fail_hr(app, L"GetDefaultAudioEndpoint(eConsole)", hr); goto done; }
    hr = get_default_id(enumerator, eCommunications, &comms_id);
    if (FAILED(hr) && hr != E_NOTFOUND) { rc = fail_hr(app, L"GetDefaultAudioEndpoint(eCommunications)", hr); goto done; }

    hr = IMMDeviceEnumerator_EnumAudioEndpoints(enumerator, eRender, DEVICE_STATE_ACTIVE, &collection);
    if (FAILED(hr)) { rc = fail_hr(app, L"EnumAudioEndpoints", hr); goto done; }
    hr = IMMDeviceCollection_GetCount(collection, &count);
    if (FAILED(hr)) { rc = fail_hr(app, L"GetCount", hr); goto done; }

    for (i = 0; i < count; i++) {
        IMMDevice *device = NULL;
        LPWSTR id = NULL;
        BOOL is_default, is_comms;

        hr = IMMDeviceCollection_Item(collection, i, &device);
        if (FAILED(hr)) { rc = fail_hr(app, L"Item", hr); goto done; }
        hr = IMMDevice_GetId(device, &id);
        if (FAILED(hr)) { IMMDevice_Release(device); rc = fail_hr(app, L"GetId", hr); goto done; }

        tb_append(&app->out, id);
        tb_append_char(&app->out, L'\t');
        hr = append_friendly_name(device, &app->out);
        IMMDevice_Release(device);
        if (FAILED(hr)) { CoTaskMemFree(id); rc = fail_hr(app, L"FriendlyName", hr); goto done; }

        is_default = ids_equal(id, default_id);
        is_comms = ids_equal(id, comms_id);
        CoTaskMemFree(id);
        tb_append_char(&app->out, L'\t');
        if (is_default) tb_append_char(&app->out, L'*');
        if (is_comms) tb_append_char(&app->out, L'c');
        if (!is_default && !is_comms) tb_append_char(&app->out, L'-');
        tb_append_char(&app->out, L'\n');
    }

done:
    if (collection) IMMDeviceCollection_Release(collection);
    CoTaskMemFree(default_id);  /* CoTaskMemFree(NULL) is a no-op */
    CoTaskMemFree(comms_id);
    return rc;
}

static int cmd_get(App *app, IMMDeviceEnumerator *enumerator)
{
    IMMDevice *device = NULL;
    LPWSTR id = NULL;
    HRESULT hr;

    hr = IMMDeviceEnumerator_GetDefaultAudioEndpoint(enumerator, eRender, eConsole, &device);
    if (hr == E_NOTFOUND) {
        tb_append(&app->err, L"error: no default playback device\n");
        return EXIT_NO_DEFAULT;
    }
    if (FAILED(hr)) return fail_hr(app, L"GetDefaultAudioEndpoint(eConsole)", hr);

    hr = IMMDevice_GetId(device, &id);
    if (FAILED(hr)) { IMMDevice_Release(device); return fail_hr(app, L"GetId", hr); }
    tb_append(&app->out, id);
    tb_append_char(&app->out, L'\t');
    CoTaskMemFree(id);

    hr = append_friendly_name(device, &app->out);
    IMMDevice_Release(device);
    if (FAILED(hr)) return fail_hr(app, L"FriendlyName", hr);
    tb_append_char(&app->out, L'\n');
    return EXIT_OK;
}

static int cmd_set(App *app, IMMDeviceEnumerator *enumerator, const WCHAR *id)
{
    LPWSTR canonical_id = NULL;
    int rc = resolve_active_endpoint(app, enumerator, id, &canonical_id);
    if (rc == EXIT_OK) rc = set_default_all_roles(app, canonical_id);
    CoTaskMemFree(canonical_id);
    return rc;
}

static int cmd_toggle(App *app, IMMDeviceEnumerator *enumerator,
                      const WCHAR *id_a, const WCHAR *id_b)
{
    LPWSTR current_id = NULL, canonical_id = NULL;
    const WCHAR *target;
    int rc;
    HRESULT hr;

    /* No default at all (E_NOTFOUND) simply means "not A", so the target is A. */
    hr = get_default_id(enumerator, eConsole, &current_id);
    if (FAILED(hr) && hr != E_NOTFOUND) return fail_hr(app, L"GetDefaultAudioEndpoint(eConsole)", hr);
    target = ids_equal(current_id, id_a) ? id_b : id_a;
    CoTaskMemFree(current_id);

    rc = resolve_active_endpoint(app, enumerator, target, &canonical_id);
    if (rc == EXIT_OK) rc = set_default_all_roles(app, canonical_id);
    if (rc == EXIT_OK) {
        tb_append(&app->out, canonical_id);
        tb_append_char(&app->out, L'\n');
    }
    CoTaskMemFree(canonical_id);
    return rc;
}

/* ------------------------------------------------------------------------- */
/* Argument handling and main flow                                           */
/* ------------------------------------------------------------------------- */

static BOOL str_equal(const WCHAR *a, const WCHAR *b)
{
    while (*a && *a == *b) { a++; b++; }
    return *a == *b;
}

/* argc/argv exclude the program name. Returns the process exit code. */
static int run(App *app, int argc, WCHAR **argv)
{
    const WCHAR *args[4];
    int nargs = 0, i, rc;
    BOOL bad_usage = FALSE;
    IMMDeviceEnumerator *enumerator = NULL;
    HRESULT hr;
    BOOL com_initialized;

    /* "--timing" may appear anywhere; everything else is positional. */
    for (i = 0; i < argc; i++) {
        if (str_equal(argv[i], L"--timing")) g_timing.enabled = TRUE;
        else if (nargs < 4) args[nargs++] = argv[i];
        else bad_usage = TRUE;
    }
    if (g_timing.enabled) {
        /* The "entry" phase is the timestamp taken by the entry point itself. */
        g_timing.names[0] = L"entry";
        g_timing.ticks[0] = g_timing.qpc_entry.QuadPart;
        g_timing.count = 1;
    }

    if (nargs == 0 || bad_usage) bad_usage = TRUE;
    else if (str_equal(args[0], L"list") || str_equal(args[0], L"get")) bad_usage = (nargs != 1);
    else if (str_equal(args[0], L"set")) bad_usage = (nargs != 2);
    else if (str_equal(args[0], L"toggle")) bad_usage = (nargs != 3);
    else bad_usage = TRUE;

    /* Usage path: no COM at all, so this measures the language runtime floor. */
    if (bad_usage) {
        tb_append(&app->err, USAGE_TEXT);
        return EXIT_USAGE;
    }

    /* STA: the product's settings dialog needs it, and both CLSIDs are
     * ThreadingModel=Both so there is no marshaling cost either way. */
    hr = CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    if (hr == RPC_E_CHANGED_MODE) {
        com_initialized = FALSE;        /* usable, but not ours to uninitialize */
    } else if (FAILED(hr)) {
        return fail_hr(app, L"CoInitializeEx", hr);
    } else {
        com_initialized = TRUE;         /* S_OK or S_FALSE: pair with CoUninitialize */
    }
    phase(L"com_init");

    hr = CoCreateInstance(&CLSID_MMDeviceEnumerator, NULL, CLSCTX_INPROC_SERVER,
                          &IID_IMMDeviceEnumerator, (void **)&enumerator);
    if (FAILED(hr)) {
        rc = fail_hr(app, L"CoCreateInstance(MMDeviceEnumerator)", hr);
    } else {
        phase(L"enumerator");
        if (str_equal(args[0], L"list")) rc = cmd_list(app, enumerator);
        else if (str_equal(args[0], L"get")) rc = cmd_get(app, enumerator);
        else if (str_equal(args[0], L"set")) rc = cmd_set(app, enumerator, args[1]);
        else rc = cmd_toggle(app, enumerator, args[1], args[2]);
        if (rc == EXIT_OK) phase(L"work_done");
        IMMDeviceEnumerator_Release(enumerator);   /* release before CoUninitialize */
    }

    if (com_initialized) CoUninitialize();
    return rc;
}

/* Shared tail of both entry points: run, flush stdout (on success only), stamp "exit", then
 * write errors + timing to stderr in a single write. */
static int run_and_report(int argc, WCHAR **argv)
{
    App app = { { NULL, 0, 0, FALSE }, { NULL, 0, 0, FALSE } };
    int rc = run(&app, argc, argv);

    if (app.out.oom || app.err.oom) rc = fail_hr(&app, L"HeapAlloc", E_OUTOFMEMORY);
    /* stdout is all-or-nothing: a command that fails part-way (for example an
     * endpoint vanishing while list or get reads its name) must not leave a
     * partial line for a harness to parse. */
    if (rc == EXIT_OK) write_std(STD_OUTPUT_HANDLE, &app.out);
    phase(L"exit");
    if (g_timing.enabled) timing_format(&app.err);
    write_std(STD_ERROR_HANDLE, &app.err);
    tb_free(&app.out);
    tb_free(&app.err);
    return rc;
}

#ifndef TA_NOCRT

/* CRT build (/MT or /MD): console subsystem uses wmainCRTStartup -> wmain;
 * the GUI-subsystem variant links with /ENTRY:wmainCRTStartup to get here too. */
int wmain(int argc, wchar_t **argv)
{
    timing_start();
    return run_and_report(argc - 1, argv + 1);
}

#else /* TA_NOCRT */

/* Minimal Windows command-line splitter (same rules as the CRT / CommandLineToArgvW):
 * whitespace separates arguments; "..." groups; 2n backslashes before a quote
 * become n backslashes and the quote toggles quoting; 2n+1 backslashes become n
 * backslashes and a literal quote; other backslashes are literal. The program
 * name (first token) uses the simpler rule: quotes delimit, no escapes.
 * Arguments are written into out_buf (at least as long as cmd) and pointers to
 * them into argv. Returns the argument count, excluding the program name. */
static int split_command_line(const WCHAR *cmd, WCHAR *out_buf, WCHAR **argv, int max_args)
{
    const WCHAR *p = cmd;
    WCHAR *o = out_buf;
    int argc = 0;

    /* Skip the program name. */
    if (*p == L'"') {
        p++;
        while (*p && *p != L'"') p++;
        if (*p == L'"') p++;
    } else {
        while (*p && *p != L' ' && *p != L'\t') p++;
    }

    for (;;) {
        BOOL in_quotes = FALSE;
        while (*p == L' ' || *p == L'\t') p++;
        if (!*p || argc >= max_args) break;
        argv[argc++] = o;
        while (*p && (in_quotes || (*p != L' ' && *p != L'\t'))) {
            if (*p == L'\\') {
                SIZE_T slashes = 0;
                while (*p == L'\\') { slashes++; p++; }
                if (*p == L'"') {
                    while (slashes >= 2) { *o++ = L'\\'; slashes -= 2; }
                    if (slashes) { *o++ = L'"'; p++; }      /* odd: literal quote */
                    else { in_quotes = !in_quotes; p++; }    /* even: quote toggles */
                } else {
                    while (slashes--) *o++ = L'\\';
                }
            } else if (*p == L'"') {
                if (in_quotes && p[1] == L'"') { *o++ = L'"'; p += 2; } /* "" inside quotes */
                else { in_quotes = !in_quotes; p++; }
            } else {
                *o++ = *p++;
            }
        }
        *o++ = L'\0';
    }
    return argc;
}

#define MAX_ARGS 16

/* No-CRT entry point (/ENTRY:entry). There is no CRT startup to return to,
 * so the process ends with ExitProcess. */
void __stdcall entry(void)
{
    const WCHAR *cmd;
    WCHAR *buf;
    WCHAR *argv[MAX_ARGS];
    int argc = 0, rc;

    timing_start();
    cmd = GetCommandLineW();
    buf = (WCHAR *)HeapAlloc(GetProcessHeap(), 0, (wstr_len(cmd) + 1) * sizeof(WCHAR));
    if (buf) argc = split_command_line(cmd, buf, argv, MAX_ARGS);
    rc = run_and_report(argc, argv);
    if (buf) HeapFree(GetProcessHeap(), 0, buf);
    ExitProcess((UINT)rc);
}

#endif /* TA_NOCRT */
