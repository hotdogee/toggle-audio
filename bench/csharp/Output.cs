// Code-page-independent output.
//
// Console.Out encodes with the console output code page (950 on a zh-TW machine), which would
// mangle device names such as "喇叭 (FiiO BTA30 PRO)". So text is collected in a UTF-16 buffer
// and written once at exit:
//   - redirected handle (pipe, file, NUL): UTF-8 bytes, no BOM, "\n" line endings, WriteFile;
//   - real console (GetConsoleMode succeeds): UTF-16 through WriteConsoleW, so CJK renders
//     whatever the console code page is.
// Console.OutputEncoding is deliberately never touched: setting it calls SetConsoleOutputCP,
// which changes the parent's console and costs time.

using System;
using System.Text.Unicode;

namespace ToggleAudio.Bench;

internal sealed unsafe class TextBuffer
{
    private char[] _chars = new char[512];
    private int _length;

    public bool IsEmpty => _length == 0;

    public TextBuffer Append(char c)
    {
        EnsureCapacity(1);
        _chars[_length++] = c;
        return this;
    }

    public TextBuffer Append(ReadOnlySpan<char> text)
    {
        EnsureCapacity(text.Length);
        text.CopyTo(_chars.AsSpan(_length));
        _length += text.Length;
        return this;
    }

    /// <summary>Appends a value as exactly 8 upper-case hex digits (HRESULT style, no prefix).</summary>
    public TextBuffer AppendHex32(uint value)
    {
        EnsureCapacity(8);
        for (int shift = 28; shift >= 0; shift -= 4)
        {
            _chars[_length++] = "0123456789ABCDEF"[(int)((value >> shift) & 0xF)];
        }

        return this;
    }

    /// <summary>Appends a value with exactly one decimal (e.g. "1234.5"), culture independent.</summary>
    public TextBuffer AppendFixed1(double value)
    {
        long tenths = (long)Math.Round(value * 10.0, MidpointRounding.AwayFromZero);
        if (tenths < 0)
        {
            Append('-');
            tenths = -tenths;
        }

        AppendUInt64((ulong)(tenths / 10));
        return Append('.').Append((char)('0' + (int)(tenths % 10)));
    }

    private void AppendUInt64(ulong value)
    {
        Span<char> digits = stackalloc char[20];
        int pos = digits.Length;
        do
        {
            digits[--pos] = (char)('0' + (int)(value % 10));
            value /= 10;
        }
        while (value != 0);

        Append(digits[pos..]);
    }

    private void EnsureCapacity(int extra)
    {
        if (_length + extra > _chars.Length)
        {
            Array.Resize(ref _chars, Math.Max(_chars.Length * 2, _length + extra));
        }
    }

    /// <summary>
    /// Writes the buffer to STD_OUTPUT_HANDLE or STD_ERROR_HANDLE and clears it. Output is
    /// silently dropped when there is no handle (e.g. a detached launch from G HUB).
    /// </summary>
    public void FlushTo(int stdHandle)
    {
        if (_length == 0)
        {
            return;
        }

        int length = _length;
        _length = 0;

        nint handle = Native.GetStdHandle(stdHandle);
        if (handle == 0 || handle == -1)
        {
            return;
        }

        fixed (char* chars = _chars)
        {
            uint mode;
            if (Native.GetConsoleMode(handle, &mode) != 0)
            {
                WriteConsole(handle, chars, length);
            }
            else
            {
                WriteUtf8(handle, new ReadOnlySpan<char>(chars, length));
            }
        }
    }

    private static void WriteConsole(nint handle, char* chars, int length)
    {
        while (length > 0)
        {
            uint written;
            if (Native.WriteConsoleW(handle, chars, (uint)length, &written, null) == 0 || written == 0)
            {
                return;
            }

            chars += written;
            length -= (int)written;
        }
    }

    private static void WriteUtf8(nint handle, ReadOnlySpan<char> text)
    {
        // Worst case is 3 UTF-8 bytes per UTF-16 code unit (surrogate pairs need 4 bytes for 2 units).
        byte[] bytes = new byte[text.Length * 3];
        Utf8.FromUtf16(text, bytes, out _, out int byteCount);

        fixed (byte* start = bytes)
        {
            byte* p = start;
            while (byteCount > 0)
            {
                uint written;
                if (Native.WriteFile(handle, p, (uint)byteCount, &written, null) == 0 || written == 0)
                {
                    return;
                }

                p += written;
                byteCount -= (int)written;
            }
        }
    }
}
