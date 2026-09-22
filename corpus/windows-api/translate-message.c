__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) int __stdcall GetMessageA(void *, void *, unsigned int, unsigned int);
__declspec(dllimport) int __stdcall TranslateMessage(const void *);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

__declspec(noreturn) void entry(void) {
    unsigned int key[8];
    unsigned int character[8];
    SetLastError(77);
    if (GetMessageA(key, 0, 0, 0) != 1 || key[1] != 0x100 || key[2] != 13)
        ExitProcess(1);
    if (TranslateMessage(key) != 1 || key[1] != 0x100 || GetLastError() != 77)
        ExitProcess(2);
    if (GetMessageA(character, 0, 0, 0) != 1 || character[0] != key[0] ||
        character[1] != 0x102 || character[2] != 13 || character[3] != key[3] ||
        character[4] != key[4] || character[5] != key[5] || character[6] != key[6] ||
        GetLastError() != 77)
        ExitProcess(3);
    ExitProcess(42);
}
