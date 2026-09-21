__declspec(dllimport) unsigned int __cdecl strlen(const char *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    const char bytes[] = {'a', (char)0x80, (char)0xff, 0, 'z', 0};
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    if (strlen(bytes) != 3 || strlen(bytes + 1) != 2) ExitProcess(1);
    if (strlen(bytes + 3) != 0 || strlen(bytes + 4) != 1) ExitProcess(2);
    if (*error != 123 || GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
