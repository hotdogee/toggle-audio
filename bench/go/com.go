// Minimal, cgo-free COM plumbing for the handful of Core Audio interfaces the
// tool needs. There are no generated bindings: every interface is called
// through its vtable with syscall.SyscallN, using the 0-based slot numbers
// documented in docs/research/core-audio-api.md (§A.2, §B.2) and
// docs/research/benchmark-method.md (§0).
//
// Memory/lifetime rules followed throughout:
//   - every interface pointer obtained here is Release()d exactly once;
//   - every string returned by IMMDevice::GetId is freed with CoTaskMemFree;
//   - every PROPVARIANT filled by IPropertyStore::GetValue is PropVariantClear()ed;
//   - all of it happens before CoUninitialize, on the thread locked in main.go.

package main

import (
	"syscall"
	"unsafe"
)

// hresult is a COM HRESULT. Negative values (high bit set) are failures.
type hresult uint32

func (h hresult) failed() bool { return int32(h) < 0 }

// HRESULT values the program reacts to.
const (
	hrRPCChangedMode hresult = 0x80010106 // CoInitializeEx: thread already in another apartment
	hrNotFound       hresult = 0x80070490 // HRESULT_FROM_WIN32(ERROR_NOT_FOUND): unknown endpoint / no default
	hrInvalidArg     hresult = 0x80070057 // GetDevice with a malformed id
	hrFail           hresult = 0x80004005 // E_FAIL: fallback when a load failure carries no Win32 code
)

// hresultFromWin32 is the HRESULT_FROM_WIN32 macro.
func hresultFromWin32(e syscall.Errno) hresult {
	if e == 0 {
		return 0
	}
	return hresult(0x80070000 | uint32(e)&0xFFFF)
}

// Win32 / Core Audio constants (mmdeviceapi.h, objbase.h, wtypes.h).
const (
	coinitApartmentThreaded = 0x2
	coinitDisableOLE1DDE    = 0x4
	clsctxInprocServer      = 0x1 // both classes are in-proc servers; same as the C reference

	eRender = 0 // EDataFlow

	eConsole        = 0 // ERole
	eMultimedia     = 1
	eCommunications = 2

	deviceStateActive = 0x1
	stgmRead          = 0x0
	vtLPWSTR          = 31 // VARTYPE of a CoTaskMem-allocated UTF-16 string
)

// Vtable slots (0-based; IUnknown occupies 0..2).
const (
	slotRelease = 2 // IUnknown::Release

	slotEnumAudioEndpoints      = 3 // IMMDeviceEnumerator
	slotGetDefaultAudioEndpoint = 4
	slotGetDevice               = 5

	slotCollectionGetCount = 3 // IMMDeviceCollection
	slotCollectionItem     = 4

	slotOpenPropertyStore = 4 // IMMDevice
	slotGetID             = 5
	slotGetState          = 6

	slotPropertyStoreGetValue = 5 // IPropertyStore

	// IPolicyConfig::SetDefaultEndpoint (verified against AudioSes.dll PDB symbols).
	slotPolicyConfigSetDefaultEndpoint = 13
	// IPolicyConfigVista::SetDefaultEndpoint (the Vista interface has no ResetDeviceFormat).
	slotPolicyConfigVistaSetDefaultEndpoint = 12
)

// guid has the in-memory layout of the Win32 GUID structure.
type guid struct {
	Data1 uint32
	Data2 uint16
	Data3 uint16
	Data4 [8]byte
}

// propertyKey has the layout of PROPERTYKEY.
type propertyKey struct {
	fmtid guid
	pid   uint32
}

// propVariant has the layout of PROPVARIANT on x64 (24 bytes: an 8-byte header
// and a 16-byte union). For VT_LPWSTR the union holds pwszVal at offset 8.
// The zero value is what PropVariantInit produces.
type propVariant struct {
	vt      uint16
	_       [3]uint16 // wReserved1..3
	pwszVal *uint16   // union member valid when vt == VT_LPWSTR (CoTaskMem memory)
	_       uintptr   // rest of the 16-byte union
}

var (
	clsidMMDeviceEnumerator = guid{0xBCDE0395, 0xE52F, 0x467C, [8]byte{0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E}}
	iidIMMDeviceEnumerator  = guid{0xA95664D2, 0x9614, 0x4F35, [8]byte{0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6}}

	// Undocumented policy-config classes in AudioSes.dll (core-audio-api.md §B.1).
	clsidPolicyConfigClient      = guid{0x870AF99C, 0x171D, 0x4F9E, [8]byte{0xAF, 0x0D, 0xE6, 0x3D, 0xF4, 0x0C, 0x2B, 0xC9}}
	iidIPolicyConfig             = guid{0xF8679F50, 0x850A, 0x41CF, [8]byte{0x9C, 0x72, 0x43, 0x0F, 0x29, 0x02, 0x90, 0xC8}}
	clsidPolicyConfigVistaClient = guid{0x294935CE, 0xF637, 0x4E7C, [8]byte{0xA4, 0x1B, 0xAB, 0x25, 0x54, 0x60, 0xB8, 0x62}}
	iidIPolicyConfigVista        = guid{0x568B9108, 0x44BF, 0x40B4, [8]byte{0x90, 0x06, 0x86, 0xAF, 0xE5, 0xB5, 0xA6, 0x20}}

	// PKEY_Device_FriendlyName: the exact string Windows Sound settings shows.
	pkeyDeviceFriendlyName = propertyKey{
		fmtid: guid{0xA45C254E, 0xDF1C, 0x4EFD, [8]byte{0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0}},
		pid:   14,
	}
)

// ole32 entry points. syscall.NewLazyDLL defers LoadLibrary until the first
// call, so the no-argument path never maps ole32.dll. ole32.dll is listed under
// HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs, so the
// loader always takes it from System32 (no search-order hijacking).
var (
	modOle32             = syscall.NewLazyDLL("ole32.dll")
	procCoInitializeEx   = modOle32.NewProc("CoInitializeEx")
	procCoUninitialize   = modOle32.NewProc("CoUninitialize")
	procCoCreateInstance = modOle32.NewProc("CoCreateInstance")
	procCoTaskMemFree    = modOle32.NewProc("CoTaskMemFree")
	procPropVariantClear = modOle32.NewProc("PropVariantClear")
)

// callProc calls an ole32 export and returns its result as an HRESULT (for
// void functions the value is meaningless and ignored by the caller).
//
// go:uintptrescapes makes the compiler move any variable whose address is
// converted to uintptr in the caller's argument list to the heap and keep it
// alive for the duration of the call, which is what makes passing &out safe
// through this wrapper (same mechanism as syscall.LazyProc.Call).
//
//go:uintptrescapes
func callProc(p *syscall.LazyProc, args ...uintptr) hresult {
	if err := p.Find(); err != nil {
		// Surface the real Win32 error (e.g. ERROR_MOD_NOT_FOUND or
		// ERROR_PROC_NOT_FOUND) as HRESULT_FROM_WIN32 in the error message.
		if dllErr, ok := err.(*syscall.DLLError); ok {
			if errno, ok := dllErr.Err.(syscall.Errno); ok && errno != 0 {
				return hresultFromWin32(errno)
			}
		}
		return hrFail
	}
	r, _, _ := syscall.SyscallN(p.Addr(), args...)
	return hresult(uint32(r))
}

// comObject is the memory layout of any COM interface pointer: the object
// starts with a pointer to its vtable. The array bound is only an upper limit
// for indexing; no slot beyond the real vtable is ever read.
type comObject struct {
	vtbl *[32]uintptr
}

// call invokes vtable slot `slot` with the interface pointer as the implicit
// first ("this") argument. See callProc for why go:uintptrescapes is needed.
//
// The longest call in this program passes 3 arguments after "this"; the
// fixed buffer avoids a heap allocation per call, and the guard below turns a
// future mistake into a loud failure instead of a silently truncated ABI.
//
//go:uintptrescapes
func (o *comObject) call(slot int, args ...uintptr) hresult {
	var buf [6]uintptr
	if len(args) > len(buf)-1 {
		panic("comObject.call: too many arguments")
	}
	buf[0] = uintptr(unsafe.Pointer(o))
	n := copy(buf[1:], args)
	r, _, _ := syscall.SyscallN(o.vtbl[slot], buf[:1+n]...)
	return hresult(uint32(r))
}

// release calls IUnknown::Release; nil receivers are ignored so it can be
// used unconditionally in cleanup paths.
func (o *comObject) release() {
	if o != nil {
		o.call(slotRelease)
	}
}

// coCreateInstance wraps CoCreateInstance(clsid, NULL, CLSCTX_INPROC_SERVER, iid, &obj).
func coCreateInstance(clsid, iid *guid) (*comObject, hresult) {
	var obj *comObject
	hr := callProc(procCoCreateInstance,
		uintptr(unsafe.Pointer(clsid)), 0, clsctxInprocServer,
		uintptr(unsafe.Pointer(iid)), uintptr(unsafe.Pointer(&obj)))
	if hr.failed() {
		return nil, hr
	}
	return obj, hr
}

// coTaskMemFree frees memory a COM method allocated for the caller.
func coTaskMemFree(p unsafe.Pointer) {
	if p != nil {
		callProc(procCoTaskMemFree, uintptr(p))
	}
}

// takeCoTaskString copies a NUL-terminated UTF-16 string allocated with
// CoTaskMemAlloc into a Go (UTF-8) string and frees the original.
func takeCoTaskString(p *uint16) string {
	s := utf16PtrToString(p)
	coTaskMemFree(unsafe.Pointer(p))
	return s
}

// utf16PtrToString converts a NUL-terminated UTF-16 string owned by foreign
// code into a Go string (UTF-8; surrogate pairs are decoded correctly).
func utf16PtrToString(p *uint16) string {
	if p == nil {
		return ""
	}
	n := 0
	for *(*uint16)(unsafe.Add(unsafe.Pointer(p), uintptr(n)*2)) != 0 {
		n++
	}
	return syscall.UTF16ToString(unsafe.Slice(p, n))
}
