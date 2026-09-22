__declspec(dllimport) int __stdcall lstrlenA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const char text[] = {'a', (char)0x80, (char)0xff, 0, 'z', 0};

void entry(void) {
    SetLastError(77);
    if (lstrlenA(text) != 3 || lstrlenA(text + 1) != 2) ExitProcess(1);
    if (lstrlenA(text + 3) != 0 || lstrlenA(0) != 0) ExitProcess(2);
    if (lstrlenA(text + 4) != 1 || GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
