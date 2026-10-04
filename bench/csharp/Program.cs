// ta-cs: C# .NET 9 NativeAOT implementation of the toggle-audio bench CLI.
//
//   ta-cs list                     active render endpoints: "<id>\t<name>\t<flags>"
//   ta-cs get                      current default (eConsole): "<id>\t<name>"
//   ta-cs set <id>                 make <id> the default for eConsole, eMultimedia, eCommunications
//   ta-cs toggle <idA> <idB>       if the default is idA set idB, otherwise set idA; prints the target
//   --timing (anywhere)            phase timestamps on stderr
//
// Exit codes: 0 OK, 1 usage, 2 COM failure, 3 device not found or not active, 4 no default device.
// The contract is defined in docs/research/benchmark-method.md section 1 and docs/DESIGN.md section 12.

using System;

namespace ToggleAudio.Bench;

internal static unsafe class Program
{
    private const int ExitOk = 0;
    private const int ExitUsage = 1;
    private const int ExitComFailure = 2;
    private const int ExitDeviceNotFound = 3;
    private const int ExitNoDefault = 4;

    private const string Usage = "usage: ta-cs list | get | set <id> | toggle <idA> <idB>  [--timing]\n";

    private static readonly TextBuffer s_stdout = new();
    private static readonly TextBuffer s_stderr = new();

    // The NativeAOT startup code initializes COM on the main thread *before* Main runs: MTA by
    // default, STA with [STAThread]. That cannot be switched off, so the real CoInitializeEx cost
    // lands in create_to_entry and the com_init phase below is near zero (our own call returns
    // S_FALSE). [STAThread] keeps the apartment identical to the other implementations (STA);
    // without it our CoInitializeEx would get RPC_E_CHANGED_MODE and run in the MTA instead.
    [STAThread]
    private static int Main(string[] args)
    {
        // First statements: the entry stamps for --timing (both are a few dozen nanoseconds).
        long entryTicks = Timing.Now();
        long entryFileTime = Timing.PreciseFileTime();

        // Split "--timing" (accepted anywhere) from the positional arguments.
        // Repeating the flag is harmless: timing is enabled once, after the loop.
        string[] positional = new string[args.Length];
        int count = 0;
        bool timingRequested = false;
        foreach (string arg in args)
        {
            if (arg == "--timing")
            {
                timingRequested = true;
            }
            else
            {
                positional[count++] = arg;
            }
        }

        if (timingRequested)
        {
            Timing.Enable(entryTicks, entryFileTime);
        }

        string command = count > 0 ? positional[0] : string.Empty;
        bool valid = (command == "list" && count == 1)
                  || (command == "get" && count == 1)
                  || (command == "set" && count == 2)
                  || (command == "toggle" && count == 3);

        int exitCode;
        if (!valid)
        {
            // Usage path: ta-cs makes no COM call, so this measures the runtime floor (which,
            // for NativeAOT, includes the runtime's own COM initialization; see above).
            s_stderr.Append(Usage);
            exitCode = ExitUsage;
        }
        else
        {
            exitCode = RunWithCom(command, positional);
        }

        s_stdout.FlushTo(Native.STD_OUTPUT_HANDLE);
        Timing.Mark("exit");
        Timing.AppendTo(s_stderr);
        s_stderr.FlushTo(Native.STD_ERROR_HANDLE);
        return exitCode;
    }

    /// <summary>
    /// COM init, enumerator creation, the command itself, then full cleanup (every interface
    /// released before CoUninitialize, as the benchmark contract requires for fairness).
    /// </summary>
    private static int RunWithCom(string command, string[] args)
    {
        int hr = Native.CoInitializeEx(null, Native.COINIT_APARTMENTTHREADED | Native.COINIT_DISABLE_OLE1DDE);
        if (hr < 0 && hr != Native.RPC_E_CHANGED_MODE)
        {
            return ComFailure("CoInitializeEx", hr);
        }

        // S_OK and S_FALSE must be paired with CoUninitialize; RPC_E_CHANGED_MODE must not.
        bool uninitialize = hr >= 0;
        Timing.Mark("com_init");

        void* enumerator = null;
        try
        {
            Guid clsid = AudioGuids.CLSID_MMDeviceEnumerator;
            Guid iid = AudioGuids.IID_IMMDeviceEnumerator;
            hr = Native.CoCreateInstance(&clsid, null, Native.CLSCTX_INPROC_SERVER, &iid, &enumerator);
            if (hr < 0)
            {
                return ComFailure("CoCreateInstance(MMDeviceEnumerator)", hr);
            }

            Timing.Mark("enumerator");

            int exitCode = command switch
            {
                "list" => List(enumerator),
                "get" => Get(enumerator),
                "set" => Set(enumerator, args[1]),
                _ => Toggle(enumerator, args[1], args[2]),
            };

            // Stamped only on success (as in ta.c): a failed command completed no work.
            if (exitCode == ExitOk)
            {
                Timing.Mark("work_done");
            }

            return exitCode;
        }
        finally
        {
            Vtbl.Release(ref enumerator);
            if (uninitialize)
            {
                Native.CoUninitialize();
            }
        }
    }

    /// <summary>
    /// list: every ACTIVE render endpoint in enumeration order, "<id>\t<name>\t<flags>", where
    /// flags is "*" (default eConsole), "c" (default eCommunications), "*c" (both) or "-".
    /// </summary>
    private static int List(void* enumerator)
    {
        int hr = Audio.TryGetDefaultId(enumerator, AudioConst.eConsole, out string? defaultConsole);
        if (hr < 0)
        {
            return ComFailure("GetDefaultAudioEndpoint(eConsole)", hr);
        }

        hr = Audio.TryGetDefaultId(enumerator, AudioConst.eCommunications, out string? defaultCommunications);
        if (hr < 0)
        {
            return ComFailure("GetDefaultAudioEndpoint(eCommunications)", hr);
        }

        void* collection = null;
        try
        {
            hr = MMDeviceEnumerator.EnumAudioEndpoints(enumerator, AudioConst.eRender, AudioConst.DEVICE_STATE_ACTIVE, &collection);
            if (hr < 0)
            {
                return ComFailure("EnumAudioEndpoints", hr);
            }

            uint count;
            hr = MMDeviceCollection.GetCount(collection, &count);
            if (hr < 0)
            {
                return ComFailure("IMMDeviceCollection::GetCount", hr);
            }

            for (uint i = 0; i < count; i++)
            {
                void* device = null;
                try
                {
                    hr = MMDeviceCollection.Item(collection, i, &device);
                    if (hr < 0)
                    {
                        return ComFailure("IMMDeviceCollection::Item", hr);
                    }

                    hr = Audio.GetId(device, out string id);
                    if (hr < 0)
                    {
                        return ComFailure("IMMDevice::GetId", hr);
                    }

                    hr = Audio.GetFriendlyName(device, out string name);
                    if (hr < 0)
                    {
                        return ComFailure("IPropertyStore::GetValue(PKEY_Device_FriendlyName)", hr);
                    }

                    bool isDefault = SameId(id, defaultConsole);
                    bool isCommunications = SameId(id, defaultCommunications);
                    s_stdout.Append(id).Append('\t').Append(name).Append('\t');
                    if (isDefault)
                    {
                        s_stdout.Append('*');
                    }

                    if (isCommunications)
                    {
                        s_stdout.Append('c');
                    }

                    if (!isDefault && !isCommunications)
                    {
                        s_stdout.Append('-');
                    }

                    s_stdout.Append('\n');
                }
                finally
                {
                    Vtbl.Release(ref device);
                }
            }

            return ExitOk;
        }
        finally
        {
            Vtbl.Release(ref collection);
        }
    }

    /// <summary>get: "<id>\t<name>" of the default render endpoint for eConsole.</summary>
    private static int Get(void* enumerator)
    {
        void* device = null;
        try
        {
            int hr = MMDeviceEnumerator.GetDefaultAudioEndpoint(enumerator, AudioConst.eRender, AudioConst.eConsole, &device);
            if (hr == Native.E_NOTFOUND)
            {
                s_stderr.Append("error: no default render device\n");
                return ExitNoDefault;
            }

            if (hr < 0)
            {
                return ComFailure("GetDefaultAudioEndpoint(eConsole)", hr);
            }

            hr = Audio.GetId(device, out string id);
            if (hr < 0)
            {
                return ComFailure("IMMDevice::GetId", hr);
            }

            hr = Audio.GetFriendlyName(device, out string name);
            if (hr < 0)
            {
                return ComFailure("IPropertyStore::GetValue(PKEY_Device_FriendlyName)", hr);
            }

            s_stdout.Append(id).Append('\t').Append(name).Append('\n');
            return ExitOk;
        }
        finally
        {
            Vtbl.Release(ref device);
        }
    }

    /// <summary>
    /// set: verify that the endpoint exists and is ACTIVE, then SetDefaultEndpoint for eConsole,
    /// eMultimedia and eCommunications, in that order. Runs even when the endpoint already is
    /// the default (the "set-noop" benchmark scenario measures exactly that).
    /// </summary>
    private static int Set(void* enumerator, string id) => Set(enumerator, id, out _);

    /// <summary>
    /// set, returning the canonical endpoint id (IMMDevice::GetId of the validated device). That
    /// id, not the user's spelling, is what SetDefaultEndpoint receives and what toggle prints,
    /// so the output matches the C reference byte for byte even if the argument's case differs.
    /// </summary>
    private static int Set(void* enumerator, string id, out string canonicalId)
    {
        int exitCode;
        fixed (char* idPtr = id) // .NET strings are NUL-terminated, so this is a valid LPCWSTR
        {
            exitCode = ValidateActive(enumerator, idPtr, id, out canonicalId);
        }

        if (exitCode != ExitOk)
        {
            return exitCode;
        }

        fixed (char* canonicalPtr = canonicalId)
        {
            return SetDefaultForAllRoles(canonicalPtr);
        }
    }

    /// <summary>SetDefaultEndpoint for eConsole, eMultimedia and eCommunications, in that order.</summary>
    private static int SetDefaultForAllRoles(char* idPtr)
    {
        int hr = Audio.CreatePolicyConfig(out void* policyConfig, out int setDefaultSlot);
        if (hr < 0)
        {
            return ComFailure("CoCreateInstance(PolicyConfigClient)", hr);
        }

        try
        {
            hr = PolicyConfig.SetDefaultEndpoint(policyConfig, setDefaultSlot, idPtr, AudioConst.eConsole);
            if (hr < 0)
            {
                return ComFailure("SetDefaultEndpoint(eConsole)", hr);
            }

            hr = PolicyConfig.SetDefaultEndpoint(policyConfig, setDefaultSlot, idPtr, AudioConst.eMultimedia);
            if (hr < 0)
            {
                return ComFailure("SetDefaultEndpoint(eMultimedia)", hr);
            }

            hr = PolicyConfig.SetDefaultEndpoint(policyConfig, setDefaultSlot, idPtr, AudioConst.eCommunications);
            if (hr < 0)
            {
                return ComFailure("SetDefaultEndpoint(eCommunications)", hr);
            }

            return ExitOk;
        }
        finally
        {
            Vtbl.Release(ref policyConfig);
        }
    }

    /// <summary>toggle: current eConsole default == idA ? set idB : set idA. Prints the canonical target id.</summary>
    private static int Toggle(void* enumerator, string idA, string idB)
    {
        int hr = Audio.TryGetDefaultId(enumerator, AudioConst.eConsole, out string? current);
        if (hr < 0)
        {
            return ComFailure("GetDefaultAudioEndpoint(eConsole)", hr);
        }

        string target = SameId(current, idA) ? idB : idA;
        int exitCode = Set(enumerator, target, out string canonicalTarget);
        if (exitCode == ExitOk)
        {
            s_stdout.Append(canonicalTarget).Append('\n');
        }

        return exitCode;
    }

    /// <summary>
    /// GetDevice + GetState + GetId. GetDevice succeeds for NOTPRESENT/DISABLED/UNPLUGGED endpoints,
    /// so the state check is what keeps SetDefaultEndpoint away from a device that cannot play.
    /// GetDevice also matches ids case-insensitively, so the canonical id is read back from the
    /// device instead of trusting the argument's spelling.
    /// </summary>
    private static int ValidateActive(void* enumerator, char* idPtr, string id, out string canonicalId)
    {
        canonicalId = string.Empty;
        void* device = null;
        try
        {
            int hr = MMDeviceEnumerator.GetDevice(enumerator, idPtr, &device);
            if (hr == Native.E_NOTFOUND || hr == Native.E_INVALIDARG)
            {
                s_stderr.Append("error: device not found: ").Append(id).Append('\n');
                return ExitDeviceNotFound;
            }

            if (hr < 0)
            {
                return ComFailure("GetDevice", hr);
            }

            uint state;
            hr = MMDevice.GetState(device, &state);
            if (hr < 0)
            {
                return ComFailure("IMMDevice::GetState", hr);
            }

            if (state != AudioConst.DEVICE_STATE_ACTIVE)
            {
                s_stderr.Append("error: device not active (state=0x").AppendHex32(state).Append("): ").Append(id).Append('\n');
                return ExitDeviceNotFound;
            }

            hr = Audio.GetId(device, out canonicalId);
            if (hr < 0)
            {
                return ComFailure("IMMDevice::GetId", hr);
            }

            return ExitOk;
        }
        finally
        {
            Vtbl.Release(ref device);
        }
    }

    /// <summary>Endpoint ids are GUID based; compare them case-insensitively (ordinal).</summary>
    private static bool SameId(string? a, string? b) =>
        a is not null && b is not null && string.Equals(a, b, StringComparison.OrdinalIgnoreCase);

    /// <summary>Records "error: <step> hr=0xXXXXXXXX" for stderr and returns exit code 2.</summary>
    private static int ComFailure(string step, int hr)
    {
        s_stderr.Append("error: ").Append(step).Append(" hr=0x").AppendHex32((uint)hr).Append('\n');
        return ExitComFailure;
    }
}
