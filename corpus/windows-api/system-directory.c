typedef unsigned long DWORD;
__declspec(dllimport) unsigned int __stdcall GetSystemDirectoryA(unsigned char *, unsigned int);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) DWORD __stdcall GetModuleFileNameA(void *, unsigned char *, DWORD);
__declspec(dllimport) void __stdcall SetLastError(DWORD);
__declspec(dllimport) DWORD __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char output[64];
static unsigned char module[64];
static const unsigned char expected[] = "C:\\Windows\\System32";

void entry(void) {
    SetLastError(77);
    if (GetSystemDirectoryA(0, 0) != 20) ExitProcess(1);
    output[0] = 0x55;
    if (GetSystemDirectoryA(output, 19) != 20 || output[0] != 0x55) ExitProcess(2);
    output[20] = 0x55;
    if (GetSystemDirectoryA(output, 20) != 19 || output[20] != 0x55) ExitProcess(3);
    for (unsigned int i = 0; i < 20; ++i) {
        if (output[i] != expected[i]) ExitProcess(4);
    }
    if (GetModuleFileNameA(GetModuleHandleA("kernel32.dll"), module, 64) != 32) ExitProcess(5);
    for (unsigned int i = 0; i < 19; ++i) {
        if (module[i] != output[i]) ExitProcess(6);
    }
    if (module[19] != '\\' || GetLastError() != 77) ExitProcess(7);
    ExitProcess(42);
}
