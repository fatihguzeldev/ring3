__declspec(dllimport) unsigned long __stdcall GetShortPathNameA(const char *, char *, unsigned long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const char file[] = "C:\\Long Folder\\Long File.bin";
static char output[40] = {'x'};

void entry(void) {
    unsigned long length;
    unsigned int i;
    SetLastError(77);
    if (GetShortPathNameA("c:\\", 0, 0) != 4) ExitProcess(1);
    if (GetShortPathNameA("c:\\", output, 3) != 4 || output[0] != 'x') ExitProcess(2);
    if (GetShortPathNameA("c:\\", output, 4) != 3 || output[0] != 'c' ||
        output[1] != ':' || output[2] != '\\' || output[3] != 0 || GetLastError() != 77) ExitProcess(3);
    length = GetShortPathNameA(file, output, sizeof output);
    if (length == 0) {
        if (GetLastError() != 3) ExitProcess(4);
    } else {
        if (length != sizeof file - 1) ExitProcess(5);
        for (i = 0; i < sizeof file; i++) if (output[i] != file[i]) ExitProcess(6);
        if (GetShortPathNameA(output, output, sizeof output) != length) ExitProcess(7);
        for (i = 0; i < sizeof file; i++) if (output[i] != file[i]) ExitProcess(8);
    }
    ExitProcess(42);
}
