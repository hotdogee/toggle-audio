package main

import (
	"reflect"
	"strconv"
	"testing"
	"unsafe"
)

const (
	idPG42UQ = "{0.0.0.00000000}.{739b3554-bfed-4d61-b407-a818b317c991}"
	idBTA30  = "{0.0.0.00000000}.{30045f40-8cfd-4441-bb89-0d13fc19b589}"
)

func TestParseArgs(t *testing.T) {
	tests := []struct {
		name   string
		args   []string
		ok     bool
		cmd    command
		ids    []string
		timing bool
	}{
		{name: "no args", args: nil, ok: false},
		{name: "only timing", args: []string{"--timing"}, ok: false, timing: true},
		{name: "unknown command", args: []string{"bogus"}, ok: false},
		{name: "case sensitive", args: []string{"LIST"}, ok: false},
		{name: "help is usage", args: []string{"--help"}, ok: false},
		{name: "list", args: []string{"list"}, ok: true, cmd: cmdList, ids: []string{}},
		{name: "list extra operand", args: []string{"list", "x"}, ok: false},
		{name: "get", args: []string{"get"}, ok: true, cmd: cmdGet, ids: []string{}},
		{name: "get timing before", args: []string{"--timing", "get"}, ok: true, cmd: cmdGet, ids: []string{}, timing: true},
		{name: "set", args: []string{"set", idPG42UQ}, ok: true, cmd: cmdSet, ids: []string{idPG42UQ}},
		{name: "set timing between", args: []string{"set", "--timing", idPG42UQ}, ok: true, cmd: cmdSet, ids: []string{idPG42UQ}, timing: true},
		{name: "set missing id", args: []string{"set"}, ok: false},
		{name: "set empty id (rejected later by GetDevice, exit 3)", args: []string{"set", ""}, ok: true, cmd: cmdSet, ids: []string{""}},
		{name: "set two ids", args: []string{"set", idPG42UQ, idBTA30}, ok: false},
		{name: "toggle", args: []string{"toggle", idPG42UQ, idBTA30}, ok: true, cmd: cmdToggle, ids: []string{idPG42UQ, idBTA30}},
		{name: "toggle timing last", args: []string{"toggle", idPG42UQ, idBTA30, "--timing"}, ok: true, cmd: cmdToggle, ids: []string{idPG42UQ, idBTA30}, timing: true},
		{name: "toggle one id", args: []string{"toggle", idPG42UQ}, ok: false},
		{name: "toggle three ids", args: []string{"toggle", "a", "b", "c"}, ok: false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			inv, ok := parseArgs(tc.args)
			if ok != tc.ok {
				t.Fatalf("parseArgs(%q) ok = %v, want %v", tc.args, ok, tc.ok)
			}
			if inv.timing != tc.timing {
				t.Errorf("timing = %v, want %v", inv.timing, tc.timing)
			}
			if hasTimingFlag(tc.args) != tc.timing {
				t.Errorf("hasTimingFlag = %v, want %v", !tc.timing, tc.timing)
			}
			if !ok {
				return
			}
			if inv.cmd != tc.cmd {
				t.Errorf("cmd = %v, want %v", inv.cmd, tc.cmd)
			}
			if !reflect.DeepEqual(inv.ids, tc.ids) {
				t.Errorf("ids = %q, want %q", inv.ids, tc.ids)
			}
		})
	}
}

func TestToggleTarget(t *testing.T) {
	tests := []struct {
		name, current, want string
	}{
		{"on A goes to B", idPG42UQ, idBTA30},
		{"on B goes to A", idBTA30, idPG42UQ},
		{"elsewhere goes to A", "{0.0.0.00000000}.{5b124733-5d8f-428c-b83c-ee05ce6467fb}", idPG42UQ},
		{"no default goes to A", "", idPG42UQ},
		{"case-insensitive match on A", "{0.0.0.00000000}.{739B3554-BFED-4D61-B407-A818B317C991}", idBTA30},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if got := toggleTarget(tc.current, idPG42UQ, idBTA30); got != tc.want {
				t.Errorf("toggleTarget(%q) = %q, want %q", tc.current, got, tc.want)
			}
		})
	}
	// Degenerate input: A == B always targets A (== B).
	if got := toggleTarget(idPG42UQ, idPG42UQ, idPG42UQ); got != idPG42UQ {
		t.Errorf("A==B: got %q", got)
	}
	// An empty idA must not match an empty current (no default).
	if got := toggleTarget("", "", idBTA30); got != "" {
		t.Errorf("empty A: got %q, want empty", got)
	}
}

func TestRoleFlags(t *testing.T) {
	tests := []struct {
		def, comm bool
		want      string
	}{
		{false, false, "-"},
		{true, false, "*"},
		{false, true, "c"},
		{true, true, "*c"},
	}
	for _, tc := range tests {
		if got := roleFlags(tc.def, tc.comm); got != tc.want {
			t.Errorf("roleFlags(%v, %v) = %q, want %q", tc.def, tc.comm, got, tc.want)
		}
	}
}

func TestOutputLinesAreUTF8WithLF(t *testing.T) {
	name := "喇叭 (FiiO BTA30 PRO)" // 喇叭 = "Speakers" in zh-TW
	got := appendListLine(nil, idBTA30, name, "-")
	want := []byte(idBTA30 + "\t\xE5\x96\x87\xE5\x8F\xAD (FiiO BTA30 PRO)\t-\n")
	if !reflect.DeepEqual(got, want) {
		t.Errorf("appendListLine = %q, want %q", got, want)
	}

	got = appendGetLine(nil, idPG42UQ, "PG42UQ (NVIDIA High Definition Audio)")
	if string(got) != idPG42UQ+"\tPG42UQ (NVIDIA High Definition Audio)\n" {
		t.Errorf("appendGetLine = %q", got)
	}
}

func TestAppendPhaseLine(t *testing.T) {
	b := appendPhaseLine(nil, "entry", 1234.56)
	b = appendPhaseLine(b, "exit", 20000)
	if want := "phase\tentry\t1234.6\nphase\texit\t20000.0\n"; string(b) != want {
		t.Errorf("got %q, want %q", b, want)
	}
}

func TestHex8(t *testing.T) {
	for _, v := range []uint32{0, 0x80070490, 0x80010106, 0xDEADBEEF, 0x1} {
		got := hex8(v)
		want := strconv.FormatUint(uint64(v), 16)
		for len(want) < 8 {
			want = "0" + want
		}
		if got != upper(want) {
			t.Errorf("hex8(%#x) = %q, want %q", v, got, upper(want))
		}
	}
}

func upper(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'a' && c <= 'f' {
			b[i] = c - 'a' + 'A'
		}
	}
	return string(b)
}

func TestHRESULTFailed(t *testing.T) {
	if hresult(0).failed() || hresult(1).failed() {
		t.Error("S_OK/S_FALSE must not be failures")
	}
	if !hrNotFound.failed() || !hrRPCChangedMode.failed() {
		t.Error("E_NOTFOUND/RPC_E_CHANGED_MODE must be failures")
	}
}

func TestHRESULTFromWin32(t *testing.T) {
	const errorNotFound, errorProcNotFound = 1168, 127
	if got := hresultFromWin32(errorNotFound); got != hrNotFound {
		t.Errorf("HRESULT_FROM_WIN32(ERROR_NOT_FOUND) = %#x, want %#x", uint32(got), uint32(hrNotFound))
	}
	if got := hresultFromWin32(errorProcNotFound); got != 0x8007007F {
		t.Errorf("HRESULT_FROM_WIN32(ERROR_PROC_NOT_FOUND) = %#x, want 0x8007007F", uint32(got))
	}
	if hresultFromWin32(0) != 0 {
		t.Error("HRESULT_FROM_WIN32(0) must be S_OK")
	}
}

// The structures passed to COM must match the Win32 x64 layouts exactly.
func TestABILayouts(t *testing.T) {
	if s := unsafe.Sizeof(guid{}); s != 16 {
		t.Errorf("sizeof(GUID) = %d, want 16", s)
	}
	if s := unsafe.Sizeof(propertyKey{}); s != 20 {
		t.Errorf("sizeof(PROPERTYKEY) = %d, want 20", s)
	}
	if s := unsafe.Sizeof(propVariant{}); s != 24 {
		t.Errorf("sizeof(PROPVARIANT) = %d, want 24", s)
	}
	if o := unsafe.Offsetof(propVariant{}.pwszVal); o != 8 {
		t.Errorf("offsetof(PROPVARIANT.pwszVal) = %d, want 8", o)
	}
}

// formatGUID renders a guid in registry format, to check the literal tables.
func formatGUID(g guid) string {
	h := func(v uint64, n int) string {
		s := strconv.FormatUint(v, 16)
		for len(s) < n {
			s = "0" + s
		}
		return upper(s)
	}
	d4 := ""
	for i, b := range g.Data4 {
		if i == 2 {
			d4 += "-"
		}
		d4 += h(uint64(b), 2)
	}
	return "{" + h(uint64(g.Data1), 8) + "-" + h(uint64(g.Data2), 4) + "-" + h(uint64(g.Data3), 4) + "-" + d4 + "}"
}

func TestGUIDLiterals(t *testing.T) {
	tests := []struct {
		name string
		g    guid
		want string
	}{
		{"CLSID_MMDeviceEnumerator", clsidMMDeviceEnumerator, "{BCDE0395-E52F-467C-8E3D-C4579291692E}"},
		{"IID_IMMDeviceEnumerator", iidIMMDeviceEnumerator, "{A95664D2-9614-4F35-A746-DE8DB63617E6}"},
		{"CLSID_PolicyConfigClient", clsidPolicyConfigClient, "{870AF99C-171D-4F9E-AF0D-E63DF40C2BC9}"},
		{"IID_IPolicyConfig", iidIPolicyConfig, "{F8679F50-850A-41CF-9C72-430F290290C8}"},
		{"CLSID_PolicyConfigVistaClient", clsidPolicyConfigVistaClient, "{294935CE-F637-4E7C-A41B-AB255460B862}"},
		{"IID_IPolicyConfigVista", iidIPolicyConfigVista, "{568B9108-44BF-40B4-9006-86AFE5B5A620}"},
		{"PKEY_Device_FriendlyName.fmtid", pkeyDeviceFriendlyName.fmtid, "{A45C254E-DF1C-4EFD-8020-67D146A850E0}"},
	}
	for _, tc := range tests {
		if got := formatGUID(tc.g); got != tc.want {
			t.Errorf("%s = %s, want %s", tc.name, got, tc.want)
		}
	}
	if pkeyDeviceFriendlyName.pid != 14 {
		t.Errorf("PKEY_Device_FriendlyName.pid = %d, want 14", pkeyDeviceFriendlyName.pid)
	}
}
