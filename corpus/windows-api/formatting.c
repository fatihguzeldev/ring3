__declspec(dllimport) int __cdecl wsprintfA(char *, const char *, ...);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    char output[128];
    const char *expected = "text/-2147483648/2147483647/4294967295/abc/ABCD/%";
    SetLastError(77);
    int count = wsprintfA(output, "%s/%d/%i/%u/%x/%X/%%", "text", (int)0x80000000, 0x7fffffff, 0xffffffffu, 0xabc, 0xabcd);
    unsigned int length = 0;
    while (expected[length]) {
        if (output[length] != expected[length]) ExitProcess(1);
        length++;
    }
    if (output[length] || count != (int)length || GetLastError() != 77) ExitProcess(2);
    if (wsprintfA(output, "empty") != 5 || output[5]) ExitProcess(3);
    ExitProcess(42);
}
