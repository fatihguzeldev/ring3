typedef long (__stdcall *WindowProc)(void *, unsigned int, unsigned int, long);
__declspec(dllimport) long __stdcall CallWindowProcA(WindowProc, void *, unsigned int, unsigned int, long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static volatile unsigned int calls;
static long __stdcall procedure(void *window, unsigned int message, unsigned int word, long parameter) {
    calls++;
    if (message) return CallWindowProcA(procedure, window, message - 1, word, parameter) + 1;
    SetLastError(word);
    return (long)window + parameter;
}

void entry(void) {
    SetLastError(88);
    if (CallWindowProcA(procedure, (void *)11, 3, 77, 19) != 33) ExitProcess(1);
    if (calls != 4 || GetLastError() != 77) ExitProcess(2);
    if (CallWindowProcA(procedure, (void *)9, 0, 55, -29) != -20) ExitProcess(3);
    if (calls != 5 || GetLastError() != 55) ExitProcess(4);
    ExitProcess(42);
}
