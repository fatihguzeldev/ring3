__declspec(dllimport) unsigned long __stdcall GetFileAttributesA(const char *);
__declspec(dllimport) int __stdcall SetCurrentDirectoryA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    if (GetFileAttributesA("C:\\") != 16 || GetLastError() != 77) ExitProcess(1);
    if (GetFileAttributesA("C:\\absent\\leaf") != 0xffffffffUL || GetLastError() != 3) ExitProcess(2);
    if (GetFileAttributesA("C:\\sample.bin") == 0xffffffffUL) {
        if (GetLastError() != 2) ExitProcess(3);
    } else {
        SetLastError(88);
        if (GetFileAttributesA("c:\\\\SAMPLE.bin") != 128 || GetLastError() != 88) ExitProcess(4);
        if (GetFileAttributesA("C:\\Folder\\") != 16) ExitProcess(5);
        if (!SetCurrentDirectoryA("Folder")) ExitProcess(6);
        if (GetFileAttributesA(".\\leaf.bin") != 128) ExitProcess(7);
        if (GetFileAttributesA("..\\sample.bin") != 128) ExitProcess(8);
        if (GetFileAttributesA("missing") != 0xffffffffUL || GetLastError() != 2) ExitProcess(9);
    }
    ExitProcess(42);
}
