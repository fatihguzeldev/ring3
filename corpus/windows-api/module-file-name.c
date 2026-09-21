__declspec(dllimport) unsigned long __stdcall GetModuleFileNameA(void *, unsigned char *, unsigned long);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned char path[260];
    SetLastError(99);
    unsigned long size = GetModuleFileNameA((void *)0, path, 260);
    if (size < 7 || path[0] != 'C' || path[1] != ':' || path[2] != '\\' || path[size]) ExitProcess(1);
    if (path[size - 4] != '.' || path[size - 3] != 'e' || path[size - 2] != 'x' || path[size - 1] != 'e') ExitProcess(2);
    if (GetLastError() != 99) ExitProcess(3);
    path[3] = '*';
    if (GetModuleFileNameA((void *)0, path, 3) != 3 || path[2] != '\\' || path[3] != '*') ExitProcess(4);
    if (GetLastError()) ExitProcess(5);
    void *module = GetModuleHandleA("kernel32.dll");
    if (!module || GetModuleFileNameA(module, path, 260) != 32 || path[20] != 'k' || path[32]) ExitProcess(6);
    path[0] = '*';
    if (GetModuleFileNameA((void *)0xdeadbeef, path, 260) || path[0] != '*' || GetLastError() != 126) ExitProcess(7);
    if (GetModuleFileNameA((void *)0, (unsigned char *)0, 0) || GetLastError()) ExitProcess(8);
    ExitProcess(42);
}
