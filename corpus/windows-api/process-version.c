__declspec(dllimport) unsigned long __stdcall GetProcessVersion(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetVersion(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    if (GetProcessVersion(0) != 0x00050001) ExitProcess(1);
    if (GetVersion() != 0x0a280105) ExitProcess(2);
    if (GetProcessVersion(0) != 0x00050001 || GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
