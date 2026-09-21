__declspec(dllimport) void *__stdcall LocalAlloc(unsigned int, unsigned int);
__declspec(dllimport) void *__stdcall LocalReAlloc(void *, unsigned int, unsigned int);
__declspec(dllimport) void *__stdcall LocalFree(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    unsigned char *p = LocalAlloc(0, 13);
    void *neighbor = LocalAlloc(0, 4096);
    if (!p || !neighbor) ExitProcess(1);
    p[0] = 42;
    p[12] = 91;
    p = LocalReAlloc(p, 8193, 0x42);
    if (!p || p[0] != 42 || p[12] != 91 || p[13] || p[8192]) ExitProcess(2);
    p = LocalReAlloc(p, 3, 0);
    if (!p || p[0] != 42) ExitProcess(3);
    p = LocalReAlloc(p, 13, 0x40);
    if (!p || p[0] != 42 || p[3] || p[12]) ExitProcess(4);
    p = LocalReAlloc(p, 0, 0);
    if (!p || LocalFree(p) || LocalFree(neighbor)) ExitProcess(5);
    if (GetLastError() != 99) ExitProcess(6);
    ExitProcess(42);
}
