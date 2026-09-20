__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

volatile unsigned long input = 35;

void entry(void) {
    SetLastError(input);
    ExitProcess(GetLastError() + 7);
}
