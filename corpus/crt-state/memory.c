__declspec(dllimport) void *__cdecl memset(void *, int, unsigned int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned int buffer[2050];

void entry(void) {
    SetLastError(77);
    if (memset(buffer + 1, 0x12a, 8192) != buffer + 1) ExitProcess(1);
    if (buffer[0] || buffer[2049] || buffer[1] != 0x2a2a2a2a || buffer[2048] != 0x2a2a2a2a) ExitProcess(2);
    if (memset(buffer + 1024, -1, 8) != buffer + 1024) ExitProcess(3);
    if (buffer[1023] != 0x2a2a2a2a || buffer[1024] != 0xffffffff || buffer[1025] != 0xffffffff || buffer[1026] != 0x2a2a2a2a) ExitProcess(4);
    if (memset(buffer + 2050, 7, 0) != buffer + 2050) ExitProcess(5);
    if (GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
