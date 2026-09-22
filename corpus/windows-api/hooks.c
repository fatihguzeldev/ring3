typedef long (__stdcall *HookProc)(int, unsigned long, long);
__declspec(dllimport) void *__stdcall SetWindowsHookExA(int, HookProc, void *, unsigned long);
__declspec(dllimport) int __stdcall UnhookWindowsHookEx(void *);
__declspec(dllimport) unsigned long __stdcall GetCurrentThreadId(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static volatile int delivered;
static long __stdcall filter(int code, unsigned long word, long parameter) {
    delivered++;
    return code + (long)word + parameter;
}

void entry(void) {
    unsigned long thread = GetCurrentThreadId();
    SetLastError(77);
    void *first = SetWindowsHookExA(-1, filter, 0, thread);
    void *second = SetWindowsHookExA(-1, filter, 0, thread);
    if (!first || !second || first == second || delivered || GetLastError() != 77) ExitProcess(1);
    if (!UnhookWindowsHookEx(first) || GetLastError() != 77) ExitProcess(2);
    if (UnhookWindowsHookEx(first) || GetLastError() != 1404) ExitProcess(3);
    if (!UnhookWindowsHookEx(second) || GetLastError() != 1404) ExitProcess(4);
    if (SetWindowsHookExA(-1, 0, 0, thread) || GetLastError() != 1427) ExitProcess(5);
    ExitProcess(42);
}
