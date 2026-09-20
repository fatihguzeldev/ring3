__declspec(dllimport) unsigned int __stdcall RegisterWindowMessageA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    unsigned int first = RegisterWindowMessageA("Ring3.Alpha");
    if (first < 0xc000 || first > 0xffff) ExitProcess(1);
    if (RegisterWindowMessageA("RING3.ALPHA") != first) ExitProcess(2);
    unsigned int second = RegisterWindowMessageA("Ring3.Beta");
    if (second < 0xc000 || second > 0xffff || second == first) ExitProcess(3);
    if (RegisterWindowMessageA("Ring3.Alpha") != first) ExitProcess(4);
    if (GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
