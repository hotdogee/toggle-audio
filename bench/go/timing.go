// --timing support. Each phase is reported in microseconds since process
// creation:
//
//	entry = GetSystemTimePreciseAsFileTime() at the start of main minus the
//	        creation time from GetProcessTimes (the "create_to_entry" cost:
//	        loader + Go runtime initialization)
//	later = entry + QueryPerformanceCounter delta since entry
//
// Lines are buffered and written to stderr in one call at the very end so the
// I/O does not distort the phases. The kernel32 procedures are resolved only
// when --timing is given.

package main

import (
	"os"
	"syscall"
	"unsafe"
)

var (
	modKernel32                        = syscall.NewLazyDLL("kernel32.dll")
	procQueryPerformanceCounter        = modKernel32.NewProc("QueryPerformanceCounter")
	procQueryPerformanceFrequency      = modKernel32.NewProc("QueryPerformanceFrequency")
	procGetSystemTimePreciseAsFileTime = modKernel32.NewProc("GetSystemTimePreciseAsFileTime")
)

// timer records phase stamps. A disabled timer turns every method into a no-op.
type timer struct {
	enabled       bool
	freq          int64   // QPC ticks per second
	entryQPC      int64   // QPC at entry
	createToEntry float64 // microseconds from process creation to entry
	lines         []byte
}

// startTimer takes the entry stamps. Call it first thing in main.
func startTimer(enabled bool) *timer {
	t := &timer{enabled: enabled}
	if !enabled {
		return t
	}
	// syscall.SyscallN is a nosplit/uintptrkeepalive function, so converting
	// &local to uintptr directly in its argument list is safe.
	var nowFT syscall.Filetime
	syscall.SyscallN(procQueryPerformanceCounter.Addr(), uintptr(unsafe.Pointer(&t.entryQPC)))
	syscall.SyscallN(procGetSystemTimePreciseAsFileTime.Addr(), uintptr(unsafe.Pointer(&nowFT)))
	syscall.SyscallN(procQueryPerformanceFrequency.Addr(), uintptr(unsafe.Pointer(&t.freq)))

	var creation, exit, kernel, user syscall.Filetime
	if self, err := syscall.GetCurrentProcess(); err == nil &&
		syscall.GetProcessTimes(self, &creation, &exit, &kernel, &user) == nil {
		t.createToEntry = float64(filetime100ns(nowFT)-filetime100ns(creation)) / 10
	}
	t.lines = make([]byte, 0, 256)
	return t
}

// filetime100ns returns a FILETIME as a count of 100 ns intervals.
func filetime100ns(ft syscall.Filetime) int64 {
	return int64(ft.HighDateTime)<<32 | int64(ft.LowDateTime)
}

// mark records phase `name` at the current time.
func (t *timer) mark(name string) {
	if !t.enabled {
		return
	}
	var now int64
	syscall.SyscallN(procQueryPerformanceCounter.Addr(), uintptr(unsafe.Pointer(&now)))
	elapsed := float64(now-t.entryQPC) * 1e6 / float64(t.freq)
	t.lines = appendPhaseLine(t.lines, name, t.createToEntry+elapsed)
}

// flush writes all buffered phase lines to stderr in a single write.
func (t *timer) flush() {
	if t.enabled && len(t.lines) > 0 {
		os.Stderr.Write(t.lines)
	}
}
