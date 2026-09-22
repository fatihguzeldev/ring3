typedef long (__stdcall *HookProc)(int, unsigned long, long);
__declspec(dllimport) void *__stdcall SetWindowsHookExA(int, HookProc, void *, unsigned long);
__declspec(dllimport) int __stdcall UnhookWindowsHookEx(void *);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
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
    void *module = GetModuleHandleA(0);
    SetLastError(77);
    void *keyboard = SetWindowsHookExA(13, filter, module, 0);
    void *another = SetWindowsHookExA(13, filter, 0, 0);
    void *message = SetWindowsHookExA(-1, filter, 0, thread);
    if (!keyboard || !another || !message || keyboard == another || keyboard == message ||
        another == message || delivered || GetLastError() != 77) ExitProcess(6);
    if (!UnhookWindowsHookEx(another) || !UnhookWindowsHookEx(message) ||
        !UnhookWindowsHookEx(keyboard) || GetLastError() != 77) ExitProcess(7);
    if (SetWindowsHookExA(13, filter, module, thread) || GetLastError() != 1429) ExitProcess(8);
    if (SetWindowsHookExA(13, 0, module, 0) || GetLastError() != 1427) ExitProcess(9);
    if (UnhookWindowsHookEx(keyboard) || GetLastError() != 1404 || delivered) ExitProcess(10);
    SetLastError(77);
    void *cbt = SetWindowsHookExA(5, filter, 0, thread);
    void *other = SetWindowsHookExA(5, filter, 0, thread);
    if (!cbt || !other || cbt == other || delivered || GetLastError() != 77) ExitProcess(11);
    if (!UnhookWindowsHookEx(cbt) || !UnhookWindowsHookEx(other) || delivered) ExitProcess(12);
    if (UnhookWindowsHookEx(cbt) || GetLastError() != 1404) ExitProcess(13);
    if (SetWindowsHookExA(5, 0, 0, thread) || GetLastError() != 1427 || delivered) ExitProcess(14);
    ExitProcess(42);
}
