typedef struct { unsigned long low, high; } LARGE_INTEGER;
__declspec(dllimport) int __stdcall QueryPerformanceFrequency(LARGE_INTEGER *);
__declspec(dllimport) int __stdcall QueryPerformanceCounter(LARGE_INTEGER *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    LARGE_INTEGER frequency, again, first, second;
    SetLastError(77);
    if (!QueryPerformanceFrequency(&frequency)) ExitProcess(1);
    if (frequency.low != 1000000000 || frequency.high != 0) ExitProcess(2);
    if (!QueryPerformanceCounter(&first) || !QueryPerformanceCounter(&second))
        ExitProcess(3);
    if (first.high > second.high ||
        (first.high == second.high && first.low > second.low)) ExitProcess(4);
    if (!QueryPerformanceFrequency(&again) || again.low != frequency.low ||
        again.high != frequency.high) ExitProcess(5);
    if (GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
