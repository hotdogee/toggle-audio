/*
 * spawnbench.c - minimal, launcher-controlled process start benchmark.
 *
 * Runs a child command N times with CreateProcessW, waits on the process
 * handle and reports the wall time of each run (QueryPerformanceCounter
 * around CreateProcessW .. WaitForSingleObject) plus summary statistics, all
 * in microseconds. It removes hyperfine as a variable and, unlike hyperfine,
 * lets you choose the console creation flags, so it can emulate how a GUI
 * launcher such as Logitech G HUB starts a console-subsystem exe.
 *
 * Usage:
 *   spawnbench [-m MODE] [-n RUNS] [-w WARMUP] [-q] [--] <exe> [args...]
 *
 *   -m, --mode     inherit     no creation flags: a console child shares
 *                              spawnbench's console (hyperfine -N equivalent)
 *                  newconsole  CREATE_NEW_CONSOLE: what a GUI parent gives a
 *                              console-subsystem child (a window may appear)
 *                  noconsole   DETACHED_PROCESS: the child gets no console
 *                  nowindow    CREATE_NO_WINDOW: a console without a window
 *                  (default: inherit)
 *   -n, --runs     measured runs (default 100)
 *   -w, --warmup   unmeasured warm-up runs first (default 5)
 *   -q, --quiet    print only the summary, not one line per run
 *
 * The child's stdin/stdout/stderr are always redirected to NUL, so output
 * cost does not depend on the terminal.
 *
 * Output (tab separated, UTF-8, "\n" line endings, to stdout):
 *   # spawnbench mode=<mode> runs=<n> warmup=<w> cmd=<command line>
 *   run    <i>    <us>    <exit code>       (one line per measured run)
 *   stat   runs|failures|min_us|median_us|mean_us|p95_us|max_us   <value>
 * "failures" counts runs with a nonzero exit code (expected to equal "runs"
 * for the no-arguments scenario, which exits 1 on purpose).
 *
 * Exit codes: 0 done, 1 usage, 2 CreateProcessW or another Win32 call failed.
 */
#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <fcntl.h>
#include <io.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>

static void usage(void)
{
    fputs("usage: spawnbench [-m inherit|newconsole|noconsole|nowindow] [-n RUNS] [-w WARMUP] [-q] "
          "[--] <exe> [args...]\n", stderr);
}

/* Appends one argument to cmd using the quoting rules that the CRT and
 * CommandLineToArgvW reverse: quote when the argument is empty or contains
 * whitespace or a quote; inside quotes, backslashes are literal except before
 * a quote (double them) and a quote becomes \". Returns FALSE on overflow. */
static BOOL append_quoted(wchar_t *cmd, size_t cap, size_t *len, const wchar_t *arg)
{
    const wchar_t *p;
    BOOL needs_quotes = (*arg == L'\0') || wcspbrk(arg, L" \t\"") != NULL;

#define PUT(ch) do { if (*len + 1 >= cap) return FALSE; cmd[(*len)++] = (ch); } while (0)
    if (*len > 0) PUT(L' ');
    if (!needs_quotes) {
        for (p = arg; *p; p++) PUT(*p);
    } else {
        PUT(L'"');
        for (p = arg; ; p++) {
            size_t slashes = 0;
            while (*p == L'\\') { slashes++; p++; }
            if (*p == L'\0') {               /* before the closing quote: double them */
                while (slashes--) { PUT(L'\\'); PUT(L'\\'); }
                break;
            }
            if (*p == L'"') {                /* before a literal quote: double + escape */
                while (slashes--) { PUT(L'\\'); PUT(L'\\'); }
                PUT(L'\\'); PUT(L'"');
            } else {
                while (slashes--) PUT(L'\\');
                PUT(*p);
            }
        }
        PUT(L'"');
    }
    cmd[*len] = L'\0';
#undef PUT
    return TRUE;
}

static int compare_ll(const void *a, const void *b)
{
    long long x = *(const long long *)a, y = *(const long long *)b;
    return (x > y) - (x < y);
}

/* Prints a duration in tenths of a microsecond as "<us>.<tenth>". */
static void print_us(long long tenths)
{
    printf("%lld.%lld", tenths / 10, tenths % 10);
}

/* Resolves the program to run to a full path once, up front, so every run
 * starts the same image without CreateProcessW re-searching for it (and so
 * forward-slash paths such as "bin/ta-c.exe" work). A name containing a path
 * separator is made absolute with GetFullPathNameW (which also turns forward
 * slashes into backslashes); a bare name is looked up with SearchPathW (application directory,
 * current directory, system directories, PATH). ".exe" is appended if needed. */
static BOOL resolve_program(const wchar_t *name, wchar_t *out, DWORD cap)
{
    DWORD n;
    if (wcspbrk(name, L"\\/:") != NULL) {
        wchar_t with_ext[MAX_PATH];
        n = GetFullPathNameW(name, cap, out, NULL);
        if (n == 0 || n >= cap) return FALSE;
        if (GetFileAttributesW(out) != INVALID_FILE_ATTRIBUTES) return TRUE;
        if (swprintf(with_ext, MAX_PATH, L"%ls.exe", name) < 0) return FALSE;
        n = GetFullPathNameW(with_ext, cap, out, NULL);
        return n != 0 && n < cap && GetFileAttributesW(out) != INVALID_FILE_ATTRIBUTES;
    }
    n = SearchPathW(NULL, name, L".exe", cap, out, NULL);
    return n != 0 && n < cap;
}

int wmain(int argc, wchar_t **argv)
{
    const wchar_t *mode = L"inherit";
    DWORD creation_flags = 0;
    long runs = 100, warmup = 5;
    BOOL quiet = FALSE;
    int first = 1, i;
    static wchar_t template_cmd[32768];   /* CreateProcessW limit is 32767 chars */
    static wchar_t cmd[32768];
    static wchar_t program[MAX_PATH];     /* resolved full path of argv[first] */
    size_t cmd_len = 0;
    long long *samples;                   /* tenths of a microsecond */
    long long sum = 0;
    long failures = 0;
    LARGE_INTEGER freq;
    SECURITY_ATTRIBUTES sa = { sizeof sa, NULL, TRUE };
    HANDLE nul;
    static char cmd_utf8[3 * 32768];      /* UTF-8 copy of the command line for the header */

    /* Binary mode: "\n" stays "\n" (no CRLF translation), matching the LF output
     * of the ta-* programs. Everything printed is ASCII or already UTF-8. */
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);

    /* ---- options ---- */
    while (first < argc && argv[first][0] == L'-') {
        const wchar_t *opt = argv[first];
        if (!wcscmp(opt, L"--")) { first++; break; }
        if (!wcscmp(opt, L"-q") || !wcscmp(opt, L"--quiet")) { quiet = TRUE; first++; continue; }
        if (first + 1 >= argc) { usage(); return 1; }
        if (!wcscmp(opt, L"-m") || !wcscmp(opt, L"--mode")) mode = argv[first + 1];
        else if (!wcscmp(opt, L"-n") || !wcscmp(opt, L"--runs")) runs = wcstol(argv[first + 1], NULL, 10);
        else if (!wcscmp(opt, L"-w") || !wcscmp(opt, L"--warmup")) warmup = wcstol(argv[first + 1], NULL, 10);
        else { usage(); return 1; }
        first += 2;
    }
    if (first >= argc || runs < 1 || runs > 1000000 || warmup < 0) { usage(); return 1; }

    if (!wcscmp(mode, L"inherit")) creation_flags = 0;
    else if (!wcscmp(mode, L"newconsole")) creation_flags = CREATE_NEW_CONSOLE;
    else if (!wcscmp(mode, L"noconsole")) creation_flags = DETACHED_PROCESS;
    else if (!wcscmp(mode, L"nowindow")) creation_flags = CREATE_NO_WINDOW;
    else { usage(); return 1; }

    if (!resolve_program(argv[first], program, MAX_PATH)) {
        /* Convert to UTF-8 ourselves: "%ls" in the "C" locale fails on non-ASCII
         * (for example CJK) paths and would truncate the message. */
        static char name_utf8[3 * 32768];
        if (WideCharToMultiByte(CP_UTF8, 0, argv[first], -1, name_utf8, (int)sizeof name_utf8, NULL, NULL) <= 0)
            name_utf8[0] = '\0';
        fprintf(stderr, "error: program not found: %s\n", name_utf8);
        return 2;
    }
    for (i = first; i < argc; i++) {
        const wchar_t *arg = (i == first) ? program : argv[i];
        if (!append_quoted(template_cmd, sizeof template_cmd / sizeof template_cmd[0], &cmd_len, arg)) {
            fputs("error: command line too long\n", stderr);
            return 1;
        }
    }

    samples = (long long *)malloc(sizeof *samples * (size_t)runs);
    if (!samples) { fputs("error: out of memory\n", stderr); return 2; }

    nul = CreateFileW(L"NUL", GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, &sa,
                      OPEN_EXISTING, 0, NULL);
    if (nul == INVALID_HANDLE_VALUE) {
        fprintf(stderr, "error: CreateFileW(NUL) failed, GetLastError=%lu\n", GetLastError());
        return 2;
    }
    QueryPerformanceFrequency(&freq);

    if (WideCharToMultiByte(CP_UTF8, 0, template_cmd, -1, cmd_utf8, (int)sizeof cmd_utf8, NULL, NULL) <= 0)
        cmd_utf8[0] = '\0';
    printf("# spawnbench mode=%ls runs=%ld warmup=%ld cmd=%s\n", mode, runs, warmup, cmd_utf8);

    /* ---- runs (negative index = warm-up) ---- */
    for (i = -(int)warmup; i < (int)runs; i++) {
        STARTUPINFOW si;
        PROCESS_INFORMATION pi;
        LARGE_INTEGER t0, t1;
        DWORD exit_code = 0;
        long long ticks, tenths;

        ZeroMemory(&si, sizeof si);
        si.cb = sizeof si;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdInput = nul;
        si.hStdOutput = nul;
        si.hStdError = nul;
        /* CreateProcessW may modify the command-line buffer, so pass a fresh copy. */
        wmemcpy(cmd, template_cmd, cmd_len + 1);

        QueryPerformanceCounter(&t0);
        if (!CreateProcessW(program, cmd, NULL, NULL, TRUE, creation_flags, NULL, NULL, &si, &pi)) {
            fflush(stdout);
            fprintf(stderr, "error: CreateProcessW failed, GetLastError=%lu\n", GetLastError());
            CloseHandle(nul);
            free(samples);
            return 2;
        }
        WaitForSingleObject(pi.hProcess, INFINITE);
        QueryPerformanceCounter(&t1);

        if (!GetExitCodeProcess(pi.hProcess, &exit_code)) exit_code = (DWORD)-1;
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);

        if (i < 0) continue;
        ticks = t1.QuadPart - t0.QuadPart;
        tenths = (ticks / freq.QuadPart) * 10000000LL + (ticks % freq.QuadPart) * 10000000LL / freq.QuadPart;
        samples[i] = tenths;
        sum += tenths;
        if (exit_code != 0) failures++;
        if (!quiet) {
            printf("run\t%d\t", i + 1);
            print_us(tenths);
            printf("\t%lu\n", exit_code);
        }
    }
    CloseHandle(nul);

    /* ---- summary ---- */
    qsort(samples, (size_t)runs, sizeof *samples, compare_ll);
    {
        long long median = (runs % 2) ? samples[runs / 2]
                                      : (samples[runs / 2 - 1] + samples[runs / 2]) / 2;
        long p95_index = (long)((runs * 95 + 99) / 100) - 1; /* nearest-rank */
        printf("stat\truns\t%ld\n", runs);
        printf("stat\tfailures\t%ld\n", failures);
        printf("stat\tmin_us\t"); print_us(samples[0]); putchar('\n');
        printf("stat\tmedian_us\t"); print_us(median); putchar('\n');
        printf("stat\tmean_us\t"); print_us(sum / runs); putchar('\n');
        printf("stat\tp95_us\t"); print_us(samples[p95_index]); putchar('\n');
        printf("stat\tmax_us\t"); print_us(samples[runs - 1]); putchar('\n');
    }
    free(samples);
    return 0;
}
