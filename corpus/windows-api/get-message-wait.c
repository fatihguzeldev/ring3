__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) int __stdcall GetMessageA(void *, void *, unsigned int, unsigned int);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned int message[8];
    for (unsigned int i = 0; i < 8; i++) message[i] = 0x5a5a5a5a;
    SetLastError(77);
    GetMessageA(message, 0, 0, 0);
    ExitProcess(99);
}
