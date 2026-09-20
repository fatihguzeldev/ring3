__declspec(dllimport) void *__stdcall LoadLibraryA(const char *);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) int __stdcall FreeLibrary(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    void *handle = LoadLibraryA("MSVCRT.DLL");
    if (!handle || handle != GetModuleHandleA("msvcrt")) ExitProcess(1);
    if (handle != LoadLibraryA("msvcrt.dll.")) ExitProcess(2);
    if (!FreeLibrary(handle) || !FreeLibrary(handle)) ExitProcess(3);
    if (GetLastError() != 99) ExitProcess(4);
    if (GetModuleHandleA("absent.dll") || GetLastError() != 126) ExitProcess(5);
    if (FreeLibrary((void *)0xdeadbeef) || GetLastError() != 6) ExitProcess(6);
    ExitProcess(42);
}
