typedef void *Handle;
__declspec(dllimport) Handle __stdcall GetModuleHandleA(const char *);
__declspec(dllimport) Handle __stdcall LoadIconA(Handle, const char *);
__declspec(dllimport) int __stdcall GetSystemMetrics(int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    Handle module = GetModuleHandleA(0);
    Handle first, second;
    if (!module || GetSystemMetrics(11) != 32 || GetSystemMetrics(12) != 32) ExitProcess(1);
    SetLastError(77);
    first = LoadIconA(module, (const char *)7);
    second = LoadIconA(module, (const char *)7);
    if (!first || first != second || GetLastError() != 77) ExitProcess(2);
    if (LoadIconA(module, (const char *)8) || GetLastError() != 1814) ExitProcess(3);
    if (LoadIconA(module, (const char *)7) != first || GetLastError() != 1814) ExitProcess(4);
    ExitProcess(42);
}
