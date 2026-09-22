__declspec(dllimport) int __stdcall GetSystemMetrics(int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    int width = GetSystemMetrics(0);
    int height = GetSystemMetrics(1);
    int caption = GetSystemMetrics(4);
    if (width != 640 || height != 480 || caption != 19) ExitProcess(5);
    if (GetSystemMetrics(16) != width) ExitProcess(6);
    if (GetSystemMetrics(17) != height - caption) ExitProcess(7);
    if (GetSystemMetrics(42) != 0) ExitProcess(8);
    if (GetSystemMetrics(11) != 32 || GetSystemMetrics(12) != 32) ExitProcess(1);
    if (GetSystemMetrics(49) != 16 || GetSystemMetrics(50) != 16) ExitProcess(2);
    static const int scroll[6] = {2, 3, 9, 10, 20, 21};
    for (unsigned int i = 0; i < 6; ++i) {
        if (GetSystemMetrics(scroll[i]) != 16) ExitProcess(4);
    }
    if (GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
