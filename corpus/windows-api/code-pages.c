__declspec(dllimport) unsigned int __stdcall GetACP(void);
__declspec(dllimport) unsigned int __stdcall GetOEMCP(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    if (GetACP() != 1252) ExitProcess(1);
    if (GetOEMCP() != 437) ExitProcess(2);
    if (GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
