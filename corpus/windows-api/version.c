__declspec(dllimport) unsigned long __stdcall GetVersion(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    if (GetVersion() != 0x0a280105) ExitProcess(1);
    if (GetVersion() != 0x0a280105 || GetLastError() != 99) ExitProcess(2);
    ExitProcess(42);
}
