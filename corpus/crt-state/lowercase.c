__declspec(dllimport) int __cdecl tolower(int);
__declspec(dllimport) int __cdecl _setmbcp(int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    int *error = _errno();
    *error = 88;
    SetLastError(77);
    for (int value = -1; value <= 255; ++value) {
        int expected = value >= 'A' && value <= 'Z' ? value + ('a' - 'A') : value;
        if (tolower(value) != expected) ExitProcess(1);
    }
    if (_setmbcp(0) || tolower(0xc4) != 0xc4 || tolower('T') != 't') ExitProcess(2);
    if (_setmbcp(1252) || tolower(0xff) != 0xff || tolower('Z') != 'z') ExitProcess(3);
    if (*error != 88 || GetLastError() != 77) ExitProcess(4);
    ExitProcess(42);
}
