typedef unsigned long DWORD;
__declspec(dllimport) unsigned char *__stdcall lstrcpyA(unsigned char *, const unsigned char *);
__declspec(dllimport) unsigned char *__stdcall lstrcatA(unsigned char *, const unsigned char *);
__declspec(dllimport) void __stdcall SetLastError(DWORD);
__declspec(dllimport) DWORD __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char output[8];
static const unsigned char source[] = {'a', 0x80, 'c', 0};
static const unsigned char suffix[] = {'X', 0};

void entry(void) {
    SetLastError(77);
    output[4] = 0x55;
    if (lstrcpyA(output, source) != output) ExitProcess(1);
    if (output[0] != 'a' || output[1] != 0x80 || output[2] != 'c' || output[3] || output[4] != 0x55) ExitProcess(2);
    if (GetLastError() != 77) ExitProcess(3);
    if (lstrcpyA(output, source + 3) != output || output[0] || output[1] != 0x80) ExitProcess(4);
    if (lstrcpyA(output, 0) || GetLastError() != 87) ExitProcess(5);
    if (lstrcpyA(0, source) || GetLastError() != 87) ExitProcess(6);
    SetLastError(77);
    lstrcpyA(output, source);
    output[5] = 0x55;
    if (lstrcatA(output, source + 3) != output || output[3]) ExitProcess(7);
    if (lstrcatA(output, suffix) != output || output[0] != 'a' || output[1] != 0x80 || output[2] != 'c' || output[3] != 'X' || output[4] || output[5] != 0x55) ExitProcess(8);
    if (GetLastError() != 77) ExitProcess(9);
    if (lstrcatA(output, 0) || GetLastError() != 87 || output[3] != 'X') ExitProcess(10);
    ExitProcess(42);
}
