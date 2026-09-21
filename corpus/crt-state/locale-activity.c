__declspec(dllimport) extern volatile long __unguarded_readlc_active;
__declspec(dllimport) extern volatile long __setlc_active;
__declspec(dllimport) long __stdcall InterlockedIncrement(volatile long *);
__declspec(dllimport) long __stdcall InterlockedDecrement(volatile long *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    if (__unguarded_readlc_active || __setlc_active) ExitProcess(1);
    if (InterlockedIncrement(&__unguarded_readlc_active) != 1 ||
        InterlockedIncrement(&__unguarded_readlc_active) != 2) ExitProcess(2);
    if (__unguarded_readlc_active != 2 || __setlc_active != 0) ExitProcess(3);
    if (InterlockedDecrement(&__unguarded_readlc_active) != 1 ||
        InterlockedDecrement(&__unguarded_readlc_active) != 0) ExitProcess(4);
    __setlc_active = 0x7fffffff;
    if (InterlockedIncrement(&__setlc_active) != (long)0x80000000 ||
        InterlockedDecrement(&__setlc_active) != 0x7fffffff) ExitProcess(5);
    __setlc_active = 0;
    if (*error != 123 || GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
