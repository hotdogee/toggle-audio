# Baselines: empty processes and the spawnbench launcher

These programs do no audio work. They exist to measure the measuring:

- **The `nop` family** gives the process-creation floor: the cost of `CreateProcess`, mapping the
  image, the loader, and process teardown. Every implementation's time is read relative to it.
- **`spawnbench`** is a tiny launcher. It times N runs of any command with
  `CreateProcessW` + `WaitForSingleObject` and lets you choose the console creation flags, so it can
  emulate how a GUI program such as Logitech G HUB starts a console-subsystem exe.

| Exe | Subsystem | Manifest | Purpose |
| --- | --- | --- | --- |
| `nop.exe` | Windows (GUI) | none | Floor for GUI-subsystem exes |
| `nop-con.exe` | console | none | Floor for console-subsystem exes |
| `nop-con-detached.exe` | console | `../common/detached.manifest` | Floor for the configuration every bench exe uses (console + `consoleAllocationPolicy=detached`). The difference from `nop-con` is the cost of having a manifest: activation-context creation by the loader. |
| `spawnbench.exe` | console | none | Launcher and timer |

## nop.c

```c
void __stdcall entry(void) { ExitProcess(0); }
```

It is compiled with `/O1 /GS- /Zl` and linked with `/NODEFAULTLIB /ENTRY:entry kernel32.lib`.
That means no CRT, no imports besides `kernel32.dll`, and an image of about 2.5 KB (4 KB with the
manifest). `/MANIFEST:NO` keeps the linker from generating a manifest for the two plain variants.
The detached variant uses `/MANIFEST:EMBED /MANIFESTINPUT:..\common\detached.manifest
/MANIFESTUAC:NO`.

## spawnbench

```
spawnbench [-m MODE] [-n RUNS] [-w WARMUP] [-q] [--] <exe> [args...]
```

| Option | Meaning |
| --- | --- |
| `-m`, `--mode` | `inherit` (default): no creation flags, so a console child shares spawnbench's console. This matches `hyperfine -N`.<br>`newconsole`: `CREATE_NEW_CONSOLE`, i.e. what a GUI parent gives a console-subsystem child that has no detached manifest, or what any child gets if the parent passes that flag explicitly. **A window may appear for each run.**<br>`noconsole`: `DETACHED_PROCESS`, so the child gets no console.<br>`nowindow`: `CREATE_NO_WINDOW`, a console without a window. |
| `-n`, `--runs` | Measured runs (default 100). |
| `-w`, `--warmup` | Unmeasured warm-up runs, done first (default 5). The first run of a new exe triggers a Defender scan. |
| `-q`, `--quiet` | Print only the summary. |

Behaviour:
- The program is resolved to a full path once, before the first run, and passed as
  `lpApplicationName`. That way the runs do not include `PATH` searching, and forward-slash paths
  such as `bench/c/bin/ta-c.exe` work.
- The child's stdin, stdout and stderr are always the `NUL` device (like hyperfine's
  `--output=null`), so terminal rendering is never part of the time.
- Each run is timed with `QueryPerformanceCounter` from just before `CreateProcessW` until
  `WaitForSingleObject` on the process handle returns.

Output (tab separated, UTF-8, `\n` line endings, stdout):

```
# spawnbench mode=inherit runs=5 warmup=2 cmd=I:\...\bench\baseline\bin\nop.exe
run	1	3050.3	0
run	2	3095.9	0
...
stat	runs	5
stat	failures	0
stat	min_us	2939.7
stat	median_us	3095.9
stat	mean_us	3114.6
stat	p95_us	3260.0
stat	max_us	3260.0
```

Each `run` line is `run <i> <microseconds> <exit code>`. `failures` counts nonzero exit codes; for
the no-arguments scenario (which exits 1 on purpose) it equals `runs`. `p95_us` is nearest-rank.
spawnbench exits with 0 when done, 1 for a usage error, and 2 when the program cannot be found or
`CreateProcessW` fails.

Examples:

```powershell
$sb = 'bench\baseline\bin\spawnbench.exe'
& $sb -q -n 200 -w 10 bench\baseline\bin\nop-con-detached.exe
& $sb -q -n 200 -w 10 bench\c\bin\ta-c.exe list
& $sb -q -m noconsole -n 200 -w 10 bench\c\bin\ta-c.exe            # runtime floor, no console
& $sb -m newconsole -n 5 -w 0 bench\baseline\bin\nop-con.exe          # G HUB-style launch; windows flash
```

It is built with `/O2 /MT /W4`, console subsystem. This is a harness, not a contestant, so it uses
the CRT freely.

## Building

Prerequisites are the same as for [`../c`](../c/README.md): Visual Studio 2022 or Build Tools with
the C++ workload, and PowerShell 7.

```powershell
pwsh -File bench\baseline\build.ps1
```

The script is non-interactive and idempotent:
- It imports the MSVC x64 environment via `..\common\vsdevenv.ps1` (`vcvars64.bat`, found with
  `vswhere`; an x86 developer prompt is not reused).
- It rebuilds everything into `bin\`.
- It checks each machine type (x64) and subsystem with `dumpbin /headers`, checks that the
  detached manifest is present exactly where expected (extracted with `mt.exe -inputresource`), and
  fails if anything imports the dynamic CRT.
- It prints sizes and imports.

## Results

The measuring agent fills in this section. Times are wall time in ms, hyperfine `-N` or
spawnbench, warm.

| Exe | hyperfine `-N` mean ± σ | median | spawnbench `inherit` median | spawnbench `noconsole` median | spawnbench `newconsole` median (≤ 5 runs) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `nop.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | n/a |
| `nop-con.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |
| `nop-con-detached.exe` | _tbd_ | _tbd_ | _tbd_ | _tbd_ | _tbd_ |

The implementer's informal smoke run (inherit mode, 100 runs, launched from Git Bash) gave
medians of about 3.1 to 3.6 ms for the `nop` variants.

## Known limitations

- **`newconsole` opens a real console window for each run.** With Windows Terminal as the default
  terminal, that may be a WT window or tab. Keep the run count small and close whatever is left open.
- **The detached manifest does not stop an explicit `CREATE_NEW_CONSOLE`.** spawnbench's `newconsole`
  mode on `nop-con-detached.exe` therefore shows what an explicit `CREATE_NEW_CONSOLE` costs. It does
  not show what G HUB does, unless G HUB also passes that flag. To see the no-window behaviour,
  launch the exe through ShellExecute (Explorer, `Start-Process`).
- **Timings include spawnbench's own `CreateProcessW` and wait overhead**, the same for every
  command. Subtract the `nop` result to get the time attributable to the program.
