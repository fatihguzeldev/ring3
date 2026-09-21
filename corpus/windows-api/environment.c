__declspec(dllimport) unsigned long __stdcall GetEnvironmentVariableA(const char *, char *, unsigned long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    char value[8];
    SetLastError(77);
    value[0] = 'x';
    if (GetEnvironmentVariableA("absent", value, 8) != 0 || value[0] != 'x' ||
        GetLastError() != 203) ExitProcess(1);
    SetLastError(77);
    if (GetEnvironmentVariableA("demo", 0, 0) == 0) ExitProcess(42);
    if (GetEnvironmentVariableA("DEMO", 0, 0) != 6 || GetLastError() != 77) ExitProcess(2);
    if (GetEnvironmentVariableA("Demo", value, 5) != 6 || value[0] != 'x') ExitProcess(3);
    value[6] = 'y';
    if (GetEnvironmentVariableA("demo", value, 8) != 5 || value[0] != 'h' ||
        value[1] != 'e' || value[2] != 'l' || value[3] != 'l' || value[4] != 'o' ||
        value[5] != 0 || value[6] != 'y') ExitProcess(4);
    if (GetEnvironmentVariableA("empty", 0, 0) != 1) ExitProcess(5);
    if (GetEnvironmentVariableA("EMPTY", value, 1) != 0 || value[0] != 0 ||
        GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
