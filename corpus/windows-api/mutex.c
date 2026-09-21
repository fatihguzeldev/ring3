typedef void *HANDLE;
__declspec(dllimport) HANDLE __stdcall CreateMutexA(void *, int, const char *);
__declspec(dllimport) unsigned long __stdcall WaitForSingleObject(HANDLE, unsigned long);
__declspec(dllimport) int __stdcall ReleaseMutex(HANDLE);
__declspec(dllimport) int __stdcall CloseHandle(HANDLE);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    HANDLE owned = CreateMutexA(0, 1, 0);
    if (!owned || GetLastError() != 0) ExitProcess(1);
    SetLastError(77);
    if (WaitForSingleObject(owned, 0) != 0 || GetLastError() != 77) ExitProcess(2);
    if (!ReleaseMutex(owned) || !ReleaseMutex(owned) || GetLastError() != 77)
        ExitProcess(3);
    if (ReleaseMutex(owned) || GetLastError() != 288) ExitProcess(4);
    if (!CloseHandle(owned)) ExitProcess(5);
    if (WaitForSingleObject(owned, 0) != 0xffffffff || GetLastError() != 6)
        ExitProcess(6);
    HANDLE available = CreateMutexA(0, 0, 0);
    if (!available || available == owned) ExitProcess(7);
    if (ReleaseMutex(available) || GetLastError() != 288) ExitProcess(8);
    SetLastError(99);
    if (WaitForSingleObject(available, 0xffffffff) != 0 || !CloseHandle(available)
        || GetLastError() != 99) ExitProcess(9);
    if (CloseHandle(available) || GetLastError() != 6) ExitProcess(10);
    ExitProcess(42);
}
