// --timing support: phase timestamps in microseconds since process creation.
//
// The entry timestamp is taken on the first statement of Main with QueryPerformanceCounter
// (sub-microsecond) and, at the same moment, GetSystemTimePreciseAsFileTime. The process
// creation time from GetProcessTimes is a FILETIME, so
//     create_to_entry = precise FILETIME at entry - creation FILETIME
// captures loader + NativeAOT runtime startup. Every later phase is create_to_entry plus the
// QPC delta since entry. Lines are buffered and written once, at exit, so the I/O does not
// distort the phases. Output format (stderr): "phase\t<name>\t<microseconds, 1 decimal>".

using System;

namespace ToggleAudio.Bench;

internal static unsafe class Timing
{
    private const int MaxPhases = 16;

    private static readonly string[] s_names = new string[MaxPhases];
    private static readonly long[] s_ticks = new long[MaxPhases];
    private static int s_count;
    private static long s_entryTicks;
    private static long s_entryFileTime;

    public static bool Enabled { get; private set; }

    /// <summary>Raw QueryPerformanceCounter value.</summary>
    public static long Now()
    {
        long ticks;
        Native.QueryPerformanceCounter(&ticks);
        return ticks;
    }

    /// <summary>Precise wall clock as a FILETIME (100 ns units since 1601).</summary>
    public static long PreciseFileTime()
    {
        long fileTime;
        Native.GetSystemTimePreciseAsFileTime(&fileTime);
        return fileTime;
    }

    /// <summary>Records the entry stamps (captured by Main before anything else) and turns timing on.</summary>
    public static void Enable(long entryTicks, long entryFileTime)
    {
        Enabled = true;
        s_entryTicks = entryTicks;
        s_entryFileTime = entryFileTime;
        AddPhase("entry", entryTicks);
    }

    /// <summary>Stamps a phase if --timing is on. Costs one QPC call and two array stores.</summary>
    public static void Mark(string phase)
    {
        if (Enabled)
        {
            AddPhase(phase, Now());
        }
    }

    private static void AddPhase(string phase, long ticks)
    {
        if (s_count < MaxPhases)
        {
            s_names[s_count] = phase;
            s_ticks[s_count] = ticks;
            s_count++;
        }
    }

    /// <summary>Formats all recorded phases into <paramref name="buffer"/>.</summary>
    public static void AppendTo(TextBuffer buffer)
    {
        if (!Enabled)
        {
            return;
        }

        long frequency;
        Native.QueryPerformanceFrequency(&frequency);

        long creation, exit, kernel, user;
        double createToEntryUs = 0;
        if (Native.GetProcessTimes(Native.CurrentProcess, &creation, &exit, &kernel, &user) != 0)
        {
            createToEntryUs = (s_entryFileTime - creation) / 10.0; // 100 ns -> us
        }

        for (int i = 0; i < s_count; i++)
        {
            double us = createToEntryUs + (s_ticks[i] - s_entryTicks) * 1_000_000.0 / frequency;
            buffer.Append("phase\t").Append(s_names[i]).Append('\t').AppendFixed1(us).Append('\n');
        }
    }
}
