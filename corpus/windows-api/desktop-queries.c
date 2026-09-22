__declspec(dllimport) void *__stdcall GetDesktopWindow(void);
__declspec(dllimport) int __stdcall IsWindow(void *);
__declspec(dllimport) void *__stdcall FindWindowA(const char *, const char *);
__declspec(dllimport) unsigned int __stdcall RegisterWindowMessageA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    void *desktop = GetDesktopWindow();
    SetLastError(77);
    if (!desktop || !IsWindow(desktop) || IsWindow(0)) ExitProcess(1);
    unsigned int atom = RegisterWindowMessageA("ExampleClass");
    if (!atom || IsWindow((void *)atom)) ExitProcess(2);
    if (FindWindowA(0, 0) || FindWindowA("ExampleClass", 0)) ExitProcess(3);
    if (FindWindowA((const char *)atom, 0) || FindWindowA(0, "Example Title")) ExitProcess(4);
    if (FindWindowA(0, "") || GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
