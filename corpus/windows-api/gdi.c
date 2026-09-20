__declspec(dllimport) void *__stdcall GetDC(void *);
__declspec(dllimport) int __stdcall ReleaseDC(void *, void *);
__declspec(dllimport) int __stdcall GetDeviceCaps(void *, int);
__declspec(dllimport) int __stdcall GetSystemMetrics(int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    void *first = GetDC(0);
    void *second = GetDC(0);
    if (!first || !second || first == second) ExitProcess(1);
    static const int indices[8] = {2, 8, 10, 12, 14, 24, 88, 90};
    static const int values[8] = {1, 640, 480, 32, 1, -1, 96, 96};
    for (unsigned int i = 0; i < 8; ++i) {
        if (GetDeviceCaps(first, indices[i]) != values[i]) ExitProcess(2);
    }
    if (GetDeviceCaps(first, 8) != GetSystemMetrics(0)) ExitProcess(3);
    if (GetDeviceCaps(first, 10) != GetSystemMetrics(1)) ExitProcess(4);
    if (ReleaseDC((void *)123, first) != 1) ExitProcess(5);
    if (GetDeviceCaps(first, 8) || ReleaseDC(0, first)) ExitProcess(6);
    if (GetDeviceCaps(second, 8) != 640 || ReleaseDC(0, second) != 1) ExitProcess(7);
    if (GetDeviceCaps(0, -1) || ReleaseDC(0, 0)) ExitProcess(8);
    if (GetLastError() != 77) ExitProcess(9);
    ExitProcess(42);
}
