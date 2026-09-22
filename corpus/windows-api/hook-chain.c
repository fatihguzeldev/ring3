typedef void *Handle;
typedef long (__stdcall *WindowProc)(Handle, unsigned int, unsigned int, long);
typedef long (__stdcall *HookProc)(int, unsigned int, long);
typedef struct { unsigned int style; WindowProc procedure; int classExtra, windowExtra; Handle instance, icon, cursor, background; const char *menu, *name; } WindowClass;
typedef struct { void *parameter; Handle instance, menu, parent; int height, width, y, x; unsigned int style; const char *name, *className; unsigned int exstyle; } Create;
typedef struct { Create *create; Handle after; } CbtCreate;
typedef struct { int left, top, right, bottom; } Rect;
__declspec(dllimport) unsigned short __stdcall RegisterClassA(const WindowClass *);
__declspec(dllimport) Handle __stdcall CreateWindowExA(unsigned long, const char *, const char *, unsigned long, int, int, int, int, Handle, Handle, Handle, void *);
__declspec(dllimport) long __stdcall DefWindowProcA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) long __stdcall CallWindowProcA(WindowProc, Handle, unsigned int, unsigned int, long);
__declspec(dllimport) int __stdcall GetWindowRect(Handle, Rect *);
__declspec(dllimport) Handle __stdcall SetWindowsHookExA(int, HookProc, Handle, unsigned long);
__declspec(dllimport) int __stdcall UnhookWindowsHookEx(Handle);
__declspec(dllimport) long __stdcall CallNextHookEx(Handle, int, unsigned int, long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static Handle current, middle;
static unsigned int visits[16], count, nested;
static const char className[] = "hook-chain";

static void record(unsigned int id) {
    if (count >= 16) ExitProcess(20);
    visits[count++] = id;
}

static long __stdcall oldest(int code, unsigned int word, long parameter) {
    record(1);
    SetLastError(77);
    if (code == -7) {
        if (word != 0x12345678 || (unsigned long)parameter != 0x87654321) ExitProcess(21);
        return (long)0x89abcdef;
    }
    if (code != 3 || !word || !parameter) ExitProcess(22);
    ((CbtCreate *)parameter)->create->width += 10;
    return CallNextHookEx((Handle)0xdeadbeef, code, word, parameter);
}

static long __stdcall removed(int code, unsigned int word, long parameter) {
    ExitProcess(23);
}

static long __stdcall added(int code, unsigned int word, long parameter) {
    if (code != 3) ExitProcess(24);
    record(2);
    return CallNextHookEx(0, code, word, parameter);
}

static long __stdcall head(int code, unsigned int word, long parameter) {
    if (code != 3) ExitProcess(25);
    record(3);
    return CallNextHookEx(current, code, word, parameter);
}

static long __stdcall forward(Handle window, unsigned int message, unsigned int word, long parameter) {
    return CallNextHookEx(0, (int)message, word, parameter);
}

static long __stdcall procedure(Handle window, unsigned int message, unsigned int word, long parameter) {
    if (nested && message == 0x24) {
        if ((unsigned long)CallNextHookEx(0, -7, 0x12345678, (long)0x87654321) != 0x89abcdef) ExitProcess(26);
    }
    return DefWindowProcA(window, message, word, parameter);
}

static Handle create(void) {
    return CreateWindowExA(0, className, "chain", 0x00ca0000, 0, 0, 200, 100, 0, 0, (Handle)0x400000, 0);
}

static long __stdcall first(int code, unsigned int word, long parameter) {
    Rect rect;
    Handle inner;
    record(4);
    if (!UnhookWindowsHookEx(current) || !UnhookWindowsHookEx(middle)) ExitProcess(27);
    if (!SetWindowsHookExA(5, added, 0, 1) || !SetWindowsHookExA(5, head, 0, 1)) ExitProcess(28);
    if ((unsigned long)CallWindowProcA(forward, 0, (unsigned int)-7, 0x12345678, (long)0x87654321) != 0x89abcdef) ExitProcess(29);
    nested = 1;
    inner = create();
    nested = 0;
    if (!inner || !GetWindowRect(inner, &rect) || rect.right != 210) ExitProcess(30);
    if (CallNextHookEx(0, code, word, parameter) || CallNextHookEx(middle, code, word, parameter)) ExitProcess(31);
    return 0;
}

void entry(void) {
    static const WindowClass wc = {3, procedure, 0, 0, (Handle)0x400000, 0, 0, 0, 0, className};
    static const unsigned int expected[] = {4, 1, 3, 2, 1, 1, 1, 1, 3, 2, 1};
    Handle outer, later;
    Rect rect;
    if (!RegisterClassA(&wc) || !SetWindowsHookExA(5, oldest, 0, 1)) ExitProcess(1);
    middle = SetWindowsHookExA(5, removed, 0, 1);
    current = SetWindowsHookExA(5, first, 0, 1);
    if (!middle || !current) ExitProcess(2);
    outer = create();
    if (!outer || !GetWindowRect(outer, &rect) || rect.right != 220) ExitProcess(3);
    later = create();
    if (!later || later == outer || !GetWindowRect(later, &rect) || rect.right != 210) ExitProcess(4);
    if (count != 11 || GetLastError() != 77) ExitProcess(5);
    for (unsigned int i = 0; i < count; i++) {
        if (visits[i] != expected[i]) ExitProcess(6);
    }
    ExitProcess(42);
}
