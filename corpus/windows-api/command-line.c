__declspec(dllimport) char *__stdcall GetCommandLineA(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    const unsigned char *line = (const unsigned char *)GetCommandLineA();
    if (!line || line != (const unsigned char *)GetCommandLineA()) ExitProcess(1);
    unsigned int length = 0;
    while (length < 256 && line[length]) ++length;
    if (!length || length == 256) ExitProcess(2);
    if (GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
