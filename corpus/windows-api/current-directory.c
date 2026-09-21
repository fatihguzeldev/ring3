__declspec(dllimport) unsigned long __stdcall GetCurrentDirectoryA(unsigned long, char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned char path[8];
    SetLastError(77);
    if (GetCurrentDirectoryA(0, 0) != 4) ExitProcess(1);
    path[0] = 'x';
    if (GetCurrentDirectoryA(3, (char *)path) != 4 || path[0] != 'x') ExitProcess(2);
    path[4] = 'y';
    if (GetCurrentDirectoryA(8, (char *)path) != 3) ExitProcess(3);
    if (path[0] != 'C' || path[1] != ':' || path[2] != '\\' || path[3] != 0 ||
        path[4] != 'y') ExitProcess(4);
    if (GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
