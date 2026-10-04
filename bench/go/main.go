// Command ta-go is the Go implementation of the toggle-audio benchmark CLI
// (docs/research/benchmark-method.md §1, with the console-subsystem +
// detached-manifest deviation from docs/DESIGN.md §12).
//
//	ta-go list                 one line per ACTIVE render endpoint: <id>\t<name>\t<flags>
//	ta-go get                  <id>\t<name> of the default (eConsole) render endpoint
//	ta-go set <id>             make <id> the default for eConsole, eMultimedia, eCommunications
//	ta-go toggle <idA> <idB>   set idB if idA is the current default, otherwise idA
//	ta-go                      usage on stderr, exit 1, no COM work (runtime floor)
//	--timing (anywhere)        phase stamps on stderr
//
// It uses no cgo and no third-party modules: COM is reached through
// syscall.NewLazyDLL("ole32.dll") and raw vtable calls (see com.go).
package main

import (
	"os"
	"runtime"
)

// COM objects created in a single-threaded apartment belong to the OS thread
// that initialized it. The Go scheduler may move goroutines between threads,
// so pin the main goroutine to the main OS thread before CoInitializeEx.
func init() {
	runtime.LockOSThread()
}

func main() {
	os.Exit(run(os.Args[1:]))
}

// run executes the command line and returns the process exit code.
//
// stdout receives UTF-8 bytes with "\n" line endings: os.Stdout writes raw
// bytes with WriteFile to redirected handles (pipe, file, NUL) and only
// converts to UTF-16 WriteConsoleW for a real console, so the console code
// page (950 on the reference machine) never affects the output.
func run(args []string) int {
	t := startTimer(hasTimingFlag(args))
	t.mark("entry")

	inv, ok := parseArgs(args)
	if !ok {
		os.Stderr.WriteString(usageLine)
		t.mark("exit")
		t.flush()
		return exitUsage
	}

	out, err := execute(inv, t)
	if len(out) > 0 {
		os.Stdout.Write(out)
	}
	code := exitOK
	if err != nil {
		os.Stderr.WriteString(err.msg + "\n")
		code = err.code
	}
	t.mark("exit")
	t.flush()
	return code
}

// execute opens the COM session, runs the command and releases everything
// (deferred close: Release + CoUninitialize) before returning.
func execute(inv invocation, t *timer) ([]byte, *exitError) {
	s, err := openAudioSession(t)
	if err != nil {
		return nil, err
	}
	defer s.close()

	var out []byte
	switch inv.cmd {
	case cmdList:
		out, err = s.list()
	case cmdGet:
		out, err = s.get()
	case cmdSet:
		_, err = s.set(inv.ids[0]) // `set` prints nothing
	case cmdToggle:
		out, err = s.toggle(inv.ids[0], inv.ids[1])
	}
	if err != nil {
		return nil, err
	}
	t.mark("work_done")
	return out, nil
}
