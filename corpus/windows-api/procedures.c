typedef unsigned long (__stdcall *Query)(void);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void *__stdcall GetProcAddress(void *, const char *);
__declspec(dllimport) unsigned long __stdcall GetVersion(void);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    void *kernel = GetModuleHandleA("kernel32.dll");
    SetLastError(77);
    Query version = (Query)GetProcAddress(kernel, "GetVersion");
    Query error = (Query)GetProcAddress(kernel, "GetLastError");
    if (!version || version != GetVersion || !error || error() != 77) ExitProcess(1);
    if (version() != GetVersion() || version() != 0x0a280105) ExitProcess(2);
    void *crt = GetModuleHandleA("msvcrt.dll");
    int *argc = (int *)GetProcAddress(crt, "__argc");
    if (!argc || *argc != 1 || GetLastError() != 77) ExitProcess(3);
    if (GetProcAddress((void *)1, "GetVersion") || GetLastError() != 126) ExitProcess(4);
    ExitProcess(42);
}
