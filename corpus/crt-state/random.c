__declspec(dllimport) void __cdecl srand(unsigned int);
__declspec(dllimport) int __cdecl rand(void);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const int expected[] = {5890, 1279, 19497, 1207, 11420, 3377, 15317, 29489, 9716, 23323};

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    if (rand() != 41 || rand() != 18467) ExitProcess(1);
    for (int repeat = 0; repeat < 2; repeat++) {
        srand(1792);
        for (int index = 0; index < 10; index++) {
            if (rand() != expected[index]) ExitProcess(2);
        }
    }
    srand(0xffffffff);
    if (rand() != 35 || rand() != 29739) ExitProcess(3);
    srand(0);
    if (rand() != 38) ExitProcess(4);
    if (*error != 123 || GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
