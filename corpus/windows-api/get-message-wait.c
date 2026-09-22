__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) int __stdcall GetMessageA(void *, void *, unsigned int, unsigned int);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned int message[8];
    for (unsigned int i = 0; i < 8; i++) message[i] = 0x5a5a5a5a;
    SetLastError(77);
    if (GetMessageA(message, 0, 0, 0) != 1 || message[0] != 0 || message[1] != 0x401 ||
        message[2] != 42 || message[3] != 0x10203040 || message[4] != 1234 ||
        message[5] != 12 || message[6] != (unsigned int)-5 || message[7] != 0 ||
        GetLastError() != 77) ExitProcess(13);
    ExitProcess(42);
}
