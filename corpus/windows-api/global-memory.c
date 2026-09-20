__declspec(dllimport) void *__stdcall GlobalAlloc(unsigned int, unsigned int);
__declspec(dllimport) void *__stdcall GlobalLock(void *);
__declspec(dllimport) int __stdcall GlobalUnlock(void *);
__declspec(dllimport) void *__stdcall GlobalFree(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    void *handle = GlobalAlloc(0x2042, 8192);
    if (!handle) ExitProcess(1);
    unsigned int *memory = GlobalLock(handle);
    if (!memory || (void *)memory == handle || memory[0] || memory[2047]) ExitProcess(2);
    memory[0] = 42;
    memory[2047] = 99;
    if (GlobalLock(handle) != memory) ExitProcess(3);
    if (!GlobalUnlock(handle) || GetLastError() != 99) ExitProcess(4);
    if (GlobalUnlock(handle) || GetLastError()) ExitProcess(5);
    if (GlobalUnlock(handle) || GetLastError() != 158) ExitProcess(6);
    memory = GlobalLock(handle);
    if (!memory || memory[0] != 42 || memory[2047] != 99) ExitProcess(7);
    if (GlobalFree(handle)) ExitProcess(8);
    handle = GlobalAlloc(2, 0);
    if (!handle || GlobalLock(handle) || GetLastError() != 157 || GlobalFree(handle)) ExitProcess(9);
    handle = GlobalAlloc(0, 0);
    if (!handle || GlobalLock(handle) != handle || !GlobalUnlock(handle) || GlobalFree(handle)) ExitProcess(10);
    ExitProcess(42);
}
