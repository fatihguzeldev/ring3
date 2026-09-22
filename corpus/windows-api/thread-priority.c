__declspec(dllimport) void *__stdcall GetCurrentThread(void);
__declspec(dllimport) int __stdcall SetThreadPriority(void *, int);
__declspec(dllimport) int __stdcall GetThreadPriority(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    static const int values[] = {-15, -2, -1, 0, 1, 2, 15};
    void *thread = GetCurrentThread();
    if (GetThreadPriority(thread) != 0) ExitProcess(1);
    SetLastError(77);
    for (unsigned int i = 0; i < sizeof(values) / sizeof(values[0]); ++i) {
        if (!SetThreadPriority(thread, values[i])) ExitProcess(2);
        if (GetThreadPriority(thread) != values[i]) ExitProcess(3);
    }
    if (GetLastError() != 77) ExitProcess(4);
    if (SetThreadPriority(0, 1) || GetLastError() != 6) ExitProcess(5);
    if (GetThreadPriority(0) != 0x7fffffff || GetLastError() != 6) ExitProcess(6);
    if (GetThreadPriority(thread) != 15) ExitProcess(7);
    ExitProcess(42);
}
