__declspec(dllimport) void *__stdcall LoadCursorA(void *, const char *);
__declspec(dllimport) void *__stdcall SetCursor(void *);
__declspec(dllimport) void *__stdcall GetCursor(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    if (GetCursor()) ExitProcess(1);
    void *arrow = LoadCursorA(0, (const char *)32512);
    void *wait = LoadCursorA(0, (const char *)32514);
    if (!arrow || !wait || arrow == wait) ExitProcess(2);
    if (LoadCursorA(0, (const char *)32514) != wait) ExitProcess(3);
    if (SetCursor(arrow) || GetCursor() != arrow) ExitProcess(4);
    if (SetCursor(wait) != arrow || GetCursor() != wait) ExitProcess(5);
    if (SetCursor(0) != wait || GetCursor()) ExitProcess(6);
    if (GetLastError() != 77) ExitProcess(7);
    if (SetCursor((void *)1) || GetLastError() != 1402 || GetCursor()) ExitProcess(8);
    ExitProcess(42);
}
