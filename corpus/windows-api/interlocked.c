__declspec(dllimport) long __stdcall InterlockedExchange(volatile long *, long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    volatile long value = (long)0x80000000;
    SetLastError(77);
    if (InterlockedExchange(&value, 42) != (long)0x80000000 || value != 42)
        ExitProcess(1);
    if (InterlockedExchange(&value, -1) != 42 || value != -1)
        ExitProcess(2);
    if (InterlockedExchange(&value, -1) != -1 || value != -1)
        ExitProcess(3);
    if (GetLastError() != 77) ExitProcess(4);
    ExitProcess(42);
}
