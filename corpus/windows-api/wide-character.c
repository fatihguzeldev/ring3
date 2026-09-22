__declspec(dllimport) int __stdcall WideCharToMultiByte(unsigned int, unsigned long, const unsigned short *, int, char *, int, const char *, int *);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    static const unsigned short source[] = {'A', 0x20ac, 0x4e2d, 0};
    static const unsigned short plain[] = {'O', 'K', 0};
    char output[8] = {11, 11, 11, 11, 11, 11, 11, 11};
    char replacement = '!';
    int used = -1;
    if (WideCharToMultiByte(0, 0, source, -1, 0, 0, 0, 0) != 4) ExitProcess(1);
    if (WideCharToMultiByte(0, 0, source, -1, output, 3, 0, &used) || GetLastError() != 122) ExitProcess(2);
    if (output[0] != 11 || used != -1) ExitProcess(3);
    if (WideCharToMultiByte(0, 0, source, -1, output, 8, &replacement, &used) != 4) ExitProcess(4);
    if (output[0] != 'A' || (unsigned char)output[1] != 0x80 || output[2] != '!' || output[3] || used != 1) ExitProcess(5);
    if (WideCharToMultiByte(1252, 0, plain, 2, output, 2, 0, &used) != 2) ExitProcess(6);
    if (output[0] != 'O' || output[1] != 'K' || output[2] != '!' || used) ExitProcess(7);
    if (WideCharToMultiByte(0, 0, plain, 0, output, 8, 0, 0) || GetLastError() != 87) ExitProcess(8);
    ExitProcess(42);
}
