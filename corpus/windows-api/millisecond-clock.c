__declspec(dllimport) unsigned long __stdcall timeGetTime(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    unsigned long first = timeGetTime();
    unsigned long second = timeGetTime();
    if (second - first > 0x7fffffffUL) ExitProcess(1);
    if (GetLastError() != 77) ExitProcess(2);
    ExitProcess(42);
}
