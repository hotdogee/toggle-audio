// Pure, side-effect-free helpers: argument parsing, the toggle decision and
// output formatting. Nothing in this file touches COM or the OS, so all of it
// is covered by cli_test.go and runs on any machine (including CI runners
// without audio hardware).

package main

import (
	"strconv"
	"strings"
)

// Process exit codes of the bench CLI contract (docs/research/benchmark-method.md §1.1).
const (
	exitOK        = 0 // success
	exitUsage     = 1 // no/unknown command or wrong argument count; no COM work done
	exitCOM       = 2 // a COM call failed; "error: <step> hr=0x%08X" is printed on stderr
	exitNotActive = 3 // the requested endpoint does not exist or is not ACTIVE
	exitNoDefault = 4 // there is no default render endpoint
)

// usageLine is printed (on stderr) for exit code 1.
const usageLine = "usage: ta-go [--timing] list | get | set <id> | toggle <idA> <idB>\n"

// timingFlag may appear anywhere in argv; it never changes stdout.
const timingFlag = "--timing"

// command identifies the requested sub-command.
type command int

const (
	cmdList command = iota + 1
	cmdGet
	cmdSet
	cmdToggle
)

// invocation is the parsed command line.
type invocation struct {
	cmd    command
	ids    []string // set: [id]; toggle: [idA, idB]; otherwise empty
	timing bool     // --timing was present
}

// hasTimingFlag reports whether --timing appears anywhere in args. It is a
// separate, allocation-free scan so main can decide whether to read the clock
// before doing anything else.
func hasTimingFlag(args []string) bool {
	for _, a := range args {
		if a == timingFlag {
			return true
		}
	}
	return false
}

// parseArgs parses the arguments that follow argv[0]. It returns ok == false
// for every usage error (no command, unknown command, wrong argument count),
// in which case the caller must exit 1 without initializing COM.
func parseArgs(args []string) (inv invocation, ok bool) {
	positional := make([]string, 0, len(args))
	for _, a := range args {
		if a == timingFlag {
			inv.timing = true
			continue
		}
		positional = append(positional, a)
	}
	if len(positional) == 0 {
		return inv, false
	}

	// Required number of operands after the command word.
	var operands int
	switch positional[0] {
	case "list":
		inv.cmd, operands = cmdList, 0
	case "get":
		inv.cmd, operands = cmdGet, 0
	case "set":
		inv.cmd, operands = cmdSet, 1
	case "toggle":
		inv.cmd, operands = cmdToggle, 2
	default:
		return invocation{timing: inv.timing}, false
	}
	if len(positional)-1 != operands {
		return invocation{timing: inv.timing}, false
	}
	// Ids are not validated here: an empty or malformed id is handed to
	// IMMDeviceEnumerator::GetDevice, which rejects it, and that maps to exit 3
	// (device not found), the same as the C reference for the same argv.
	inv.ids = positional[1:]
	return inv, true
}

// toggleTarget implements the bench toggle rule: if the current default is A,
// switch to B; in every other case (current is B, something else, or there is
// no default at all, current == "") switch to A. Endpoint ids are GUID-based
// strings, so the comparison ignores ASCII case to tolerate hand-typed ids.
func toggleTarget(current, idA, idB string) string {
	if current != "" && strings.EqualFold(current, idA) {
		return idB
	}
	return idA
}

// roleFlags renders the third column of `list`: "*" default (eConsole),
// "c" default communications device, "*c" both, "-" neither.
func roleFlags(isDefault, isCommunications bool) string {
	switch {
	case isDefault && isCommunications:
		return "*c"
	case isDefault:
		return "*"
	case isCommunications:
		return "c"
	default:
		return "-"
	}
}

// appendListLine appends one `list` line: "<id>\t<name>\t<flags>\n".
// Go strings are UTF-8, so the bytes are exactly what the contract requires.
func appendListLine(b []byte, id, name, flags string) []byte {
	b = append(b, id...)
	b = append(b, '\t')
	b = append(b, name...)
	b = append(b, '\t')
	b = append(b, flags...)
	return append(b, '\n')
}

// appendGetLine appends the `get` line: "<id>\t<name>\n".
func appendGetLine(b []byte, id, name string) []byte {
	b = append(b, id...)
	b = append(b, '\t')
	b = append(b, name...)
	return append(b, '\n')
}

// appendPhaseLine appends one --timing line: "phase\t<name>\t<µs, 1 decimal>\n".
func appendPhaseLine(b []byte, name string, micros float64) []byte {
	b = append(b, "phase\t"...)
	b = append(b, name...)
	b = append(b, '\t')
	b = strconv.AppendFloat(b, micros, 'f', 1, 64)
	return append(b, '\n')
}

// hex8 formats v as 8 upper-case hex digits (the C "%08X" of an HRESULT)
// without pulling the fmt package into the binary.
func hex8(v uint32) string {
	const digits = "0123456789ABCDEF"
	var buf [8]byte
	for i := 7; i >= 0; i-- {
		buf[i] = digits[v&0xF]
		v >>= 4
	}
	return string(buf[:])
}
