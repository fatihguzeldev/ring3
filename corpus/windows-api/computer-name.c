__declspec(dllimport) int __stdcall GetComputerNameA(char *, unsigned long *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned long length = 0;
    unsigned char output[8] = {'?', '?', '?', '?', '?', '?', '?', '?'};
    if (GetComputerNameA(0, &length) || length != 6 || GetLastError() != 111)
        ExitProcess(1);
    length = 5;
    if (GetComputerNameA((char *)output, &length) || length != 6 || output[0] != '?')
        ExitProcess(2);
    SetLastError(77);
    if (!GetComputerNameA((char *)output, &length) || length != 5 || GetLastError() != 77)
        ExitProcess(3);
    if (output[0] != 'R' || output[1] != 'I' || output[2] != 'N' ||
        output[3] != 'G' || output[4] != '3' || output[5] != 0 || output[6] != '?')
        ExitProcess(4);
    ExitProcess(42);
}
