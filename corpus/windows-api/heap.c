__declspec(dllimport) void *__stdcall LocalAlloc(unsigned int, unsigned int);
__declspec(dllimport) void *__stdcall LocalFree(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    unsigned int *memory = LocalAlloc(0x40, 8192);
    if (!memory) ExitProcess(1);
    if (memory[0] || memory[2047]) ExitProcess(2);
    memory[0] = 42;
    memory[2047] = 99;
    if (memory[0] != 42 || memory[2047] != 99) ExitProcess(3);
    if (LocalFree(memory)) ExitProcess(4);
    memory = LocalAlloc(0, 0);
    if (!memory || LocalFree(memory)) ExitProcess(5);
    if (LocalFree((void *)0) || GetLastError() != 99) ExitProcess(6);
    ExitProcess(42);
}
