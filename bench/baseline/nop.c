/*
 * nop.c - the "empty process" baseline for the toggle-audio benchmarks.
 *
 * No CRT, no imports besides kernel32, no work: the entry point exits at once.
 * Timing this exe gives the process-creation floor (CreateProcess, image
 * mapping, loader, process teardown) plus the launcher's own overhead, which
 * every other measurement is compared against.
 *
 * The same source is linked three ways by build.ps1:
 *   nop.exe               Windows (GUI) subsystem, no manifest
 *   nop-con.exe           console subsystem, no manifest
 *   nop-con-detached.exe  console subsystem + ..\common\detached.manifest
 *                         (consoleAllocationPolicy=detached)
 * so the cost of the console subsystem and of the manifest can be isolated.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

/* Linked with /ENTRY:entry /NODEFAULTLIB: this is the very first user-mode
 * code of the process. There is no CRT to return to, so call ExitProcess. */
void __stdcall entry(void)
{
    ExitProcess(0);
}
