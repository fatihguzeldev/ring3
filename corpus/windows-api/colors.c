__declspec(dllimport) unsigned long __stdcall GetSysColor(int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    static const unsigned long expected[31] = {
        0xc8d0d4, 0xa56e3a, 0x6a240a, 0x808080, 0xc8d0d4,
        0xffffff, 0, 0, 0, 0xffffff, 0xc8d0d4, 0xc8d0d4,
        0x808080, 0x6a240a, 0xffffff, 0xc8d0d4, 0x808080,
        0x808080, 0, 0xc8d0d4, 0xffffff, 0x404040, 0xc8d0d4,
        0, 0xe1ffff, 0, 0xc80000, 0xf0caa6, 0xc0c0c0, 0x6a240a, 0xc8d0d4,
    };
    SetLastError(77);
    for (int index = 0; index < 31; ++index) {
        if (index != 25 && GetSysColor(index) != expected[index]) ExitProcess(1);
    }
    if (GetSysColor(-1) || GetSysColor(31)) ExitProcess(2);
    if (GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
