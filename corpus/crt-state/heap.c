__declspec(dllimport) void *__cdecl malloc(unsigned int);
__declspec(dllimport) void __cdecl free(void *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    unsigned int *data = malloc(8192);
    if (!data || ((unsigned int)data & 7)) ExitProcess(1);
    data[0] = 42;
    data[2047] = 99;
    if (data[0] != 42 || data[2047] != 99) ExitProcess(2);
    free(data);
    void *empty = malloc(0);
    if (!empty) ExitProcess(3);
    free(empty);
    free(0);
    if (*error != 123 || GetLastError() != 77) ExitProcess(4);
    if (malloc(0xffffffff) || *error != 12 || GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
