/*
 * ta-mingw.c - builds the C reference (../c/ta.c) unchanged with zig cc
 * (Clang + MinGW-w64 headers, CRT and import libraries) as ta-zigcc.exe.
 *
 * Why a wrapper: with <initguid.h> in effect, MinGW-w64's <mmdeviceapi.h>
 * *defines* CLSID_MMDeviceEnumerator and IID_IMMDeviceEnumerator itself,
 * whereas the Windows SDK header only declares them. ta.c defines both (as the
 * SDK requires), which would be a redefinition error here.
 *
 * The fix, without touching ta.c: include the headers first (same macros as
 * ta.c, so the result is identical), then rename ta.c's two GUID objects with
 * the preprocessor. ta.c's own #includes are no-ops afterwards (include
 * guards), its DEFINE_GUID lines define the renamed objects with the same
 * values, and its uses refer to the renamed objects. Everything else compiles
 * as written; ta.c's wmain entry point needs -municode with MinGW.
 */

#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#define COBJMACROS
#define WIN32_LEAN_AND_MEAN

#include <windows.h>
#include <initguid.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>

#define CLSID_MMDeviceEnumerator ta_CLSID_MMDeviceEnumerator
#define IID_IMMDeviceEnumerator ta_IID_IMMDeviceEnumerator

#include "../c/ta.c"
