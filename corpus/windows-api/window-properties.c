typedef void *Handle;
typedef long (__stdcall *WindowProc)(Handle, unsigned int, unsigned int, long);
typedef struct { unsigned int style; WindowProc procedure; int classExtra, windowExtra; Handle instance, icon, cursor, background; const char *menu, *name; } WindowClass;
__declspec(dllimport) unsigned short __stdcall RegisterClassA(const WindowClass *);
__declspec(dllimport) Handle __stdcall CreateWindowExA(unsigned long, const char *, const char *, unsigned long, int, int, int, int, Handle, Handle, Handle, void *);
__declspec(dllimport) long __stdcall DefWindowProcA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) long __stdcall CallWindowProcA(WindowProc, Handle, unsigned int, unsigned int, long);
__declspec(dllimport) Handle __stdcall GetParent(Handle);
__declspec(dllimport) long __stdcall GetWindowLongA(Handle, int);
__declspec(dllimport) long __stdcall SetWindowLongA(Handle, int, long);
__declspec(dllimport) Handle __stdcall SetWindowsHookExA(int, long (__stdcall *)(int, unsigned int, long), Handle, unsigned long);
__declspec(dllimport) int __stdcall UnhookWindowsHookEx(Handle);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static WindowProc previous;
static unsigned int messages[8], count, earlyCalls, lateCalls;
static const char className[] = "subclass-test";

static long __stdcall original(Handle window, unsigned int message, unsigned int word, long parameter) {
    if (count >= 8) ExitProcess(10);
    messages[count++] = message;
    return DefWindowProcA(window, message, word, parameter);
}

static long __stdcall late(Handle window, unsigned int message, unsigned int word, long parameter) {
    lateCalls++;
    return CallWindowProcA(previous, window, message, word, parameter);
}

static long __stdcall early(Handle window, unsigned int message, unsigned int word, long parameter) {
    earlyCalls++;
    if (message == 0x81 && SetWindowLongA(window, -4, (long)late) != (long)early) ExitProcess(11);
    return CallWindowProcA(previous, window, message, word, parameter);
}

static long __stdcall hook(int code, unsigned int word, long parameter) {
    Handle window = (Handle)word;
    SetLastError(77);
    if (code != 3 || GetParent(window) || GetLastError() != 77) ExitProcess(12);
    if (GetWindowLongA(window, -4) != (long)original) ExitProcess(13);
    previous = (WindowProc)SetWindowLongA(window, -4, (long)early);
    if (previous != original || GetWindowLongA(window, -4) != (long)early || GetLastError() != 77) ExitProcess(14);
    return 0;
}

void entry(void) {
    static const WindowClass wc = {3, original, 0, 0, (Handle)0x400000, 0, 0, 0, 0, className};
    Handle first, second, hookHandle;
    if (!RegisterClassA(&wc)) ExitProcess(1);
    hookHandle = SetWindowsHookExA(5, hook, 0, 1);
    if (!hookHandle) ExitProcess(2);
    first = CreateWindowExA(0, className, "first", 0x00ca0000, 0, 0, 200, 100, 0, 0, (Handle)0x400000, 0);
    if (!first || GetWindowLongA(first, -4) != (long)late || earlyCalls != 2 || lateCalls != 2) ExitProcess(3);
    if (!UnhookWindowsHookEx(hookHandle)) ExitProcess(4);
    second = CreateWindowExA(0, className, "second", 0x00ca0000, 0, 0, 200, 100, 0, 0, (Handle)0x400000, 0);
    if (!second || first == second || GetWindowLongA(second, -4) != (long)original) ExitProcess(5);
    if (GetParent(first) || GetParent(second) || count != 8 || GetLastError() != 77) ExitProcess(6);
    for (unsigned int i = 0; i < 8; i += 4) {
        if (messages[i] != 0x24 || messages[i+1] != 0x81 || messages[i+2] != 0x83 || messages[i+3] != 1) ExitProcess(7);
    }
    if (SetWindowLongA(first, -4, (long)original) != (long)late) ExitProcess(8);
    if (GetWindowLongA(first, -4) != (long)original) ExitProcess(9);
    ExitProcess(42);
}
