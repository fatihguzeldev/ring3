__declspec(dllimport) void *__stdcall GetCurrentThread(void);
__declspec(dllimport) unsigned long __stdcall GetCurrentThreadId(void);
__declspec(dllimport) void __stdcall InitializeCriticalSection(void *);
__declspec(dllimport) void __stdcall EnterCriticalSection(void *);
__declspec(dllimport) void __stdcall LeaveCriticalSection(void *);
__declspec(dllimport) void __stdcall DeleteCriticalSection(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    void *thread = GetCurrentThread();
    unsigned long id = GetCurrentThreadId();
    if ((unsigned long)thread != 0xfffffffe || !id) ExitProcess(1);
    if (thread != GetCurrentThread() || id != GetCurrentThreadId()) ExitProcess(2);
    unsigned long section[6];
    InitializeCriticalSection(section);
    EnterCriticalSection(section);
    if (section[3] != id) ExitProcess(3);
    LeaveCriticalSection(section);
    DeleteCriticalSection(section);
    if (GetLastError() != 99) ExitProcess(4);
    ExitProcess(42);
}
