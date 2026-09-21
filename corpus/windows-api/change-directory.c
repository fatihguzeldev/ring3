__declspec(dllimport) int __stdcall SetCurrentDirectoryA(const char *);
__declspec(dllimport) unsigned long __stdcall GetCurrentDirectoryA(unsigned long, char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned char path[8];
    SetLastError(77);
    if (!SetCurrentDirectoryA(".")) ExitProcess(1);
    if (!SetCurrentDirectoryA("C:\\")) ExitProcess(2);
    if (!SetCurrentDirectoryA("\\..")) ExitProcess(3);
    if (GetLastError() != 77) ExitProcess(4);
    if (SetCurrentDirectoryA("absent") || GetLastError() != 3) ExitProcess(5);
    if (SetCurrentDirectoryA("a*b") || GetLastError() != 123) ExitProcess(6);
    if (GetCurrentDirectoryA(8, (char *)path) != 3 || path[0] != 'C' ||
        path[1] != ':' || path[2] != '\\' || path[3] != 0) ExitProcess(7);
    if (!SetCurrentDirectoryA("C:\\.\\..\\") || GetLastError() != 123) ExitProcess(8);
    ExitProcess(42);
}
