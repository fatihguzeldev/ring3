typedef void *Handle;
typedef long (__stdcall *WindowProc)(Handle, unsigned int, unsigned int, long);
typedef struct { unsigned int style; WindowProc procedure; int classExtra, windowExtra; Handle instance, icon, cursor, background; const char *menu, *name; } WindowClass;
typedef struct { long left, top, right, bottom; } Rect;
typedef struct { void *parameter; Handle instance, menu, parent; int height, width, y, x; long style; const char *title, *name; unsigned long exstyle; } Creation;
typedef struct { Creation *creation; Handle after; } CbtCreation;
__declspec(dllimport) unsigned short __stdcall RegisterClassA(const WindowClass *);
__declspec(dllimport) int __stdcall UnregisterClassA(const char *, Handle);
__declspec(dllimport) Handle __stdcall CreateWindowExA(unsigned long, const char *, const char *, unsigned long, int, int, int, int, Handle, Handle, Handle, void *);
__declspec(dllimport) long __stdcall DefWindowProcA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) int __stdcall IsWindow(Handle);
__declspec(dllimport) Handle __stdcall FindWindowA(const char *, const char *);
__declspec(dllimport) Handle __stdcall GetActiveWindow(void);
__declspec(dllimport) int __stdcall ShowWindow(Handle, int);
__declspec(dllimport) int __stdcall UpdateWindow(Handle);
__declspec(dllimport) long __stdcall GetWindowLongA(Handle, int);
__declspec(dllimport) int __stdcall GetWindowRect(Handle, Rect *);
__declspec(dllimport) int __stdcall GetClientRect(Handle, Rect *);
__declspec(dllimport) Handle __stdcall SetWindowsHookExA(int, long (__stdcall *)(int, unsigned int, long), Handle, unsigned long);
__declspec(dllimport) int __stdcall UnhookWindowsHookEx(Handle);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const char className[] = "window-test";
static unsigned int messages[32], count, mode, nesting, hooks;
static Handle nested;

static Handle create(const char *title) {
    return CreateWindowExA(0, className, title, 0x00ca0000, 10, 20, 130, 90, 0, 0, (Handle)0x400000, (void *)1234);
}

static long __stdcall hook(int code, unsigned int word, long parameter) {
    CbtCreation *data = (CbtCreation *)parameter;
    Rect rect;
    if (code != 3 || !IsWindow((Handle)word) || data->after) ExitProcess(10);
    if (!GetWindowRect((Handle)word, &rect) || rect.left || rect.top || rect.right || rect.bottom) ExitProcess(11);
    if (data->creation->parameter != (void *)1234 || data->creation->width != 130) ExitProcess(12);
    if (UnregisterClassA(className, (Handle)0x400000) || GetLastError() != 1412) ExitProcess(13);
    data->creation->width = 220;
    data->creation->height = 180;
    hooks++;
    SetLastError(77);
    return mode == 3;
}

static long __stdcall procedure(Handle window, unsigned int message, unsigned int word, long parameter) {
    Rect rect;
    if (!IsWindow(window) || word || count >= 32) ExitProcess(20);
    messages[count++] = message;
    if (message == 0x81) {
        if (!GetClientRect(window, &rect) || rect.right != 220 || rect.bottom != 180) ExitProcess(21);
        if (mode == 1) return 0;
    }
    if (message == 1) {
        if (!GetClientRect(window, &rect) || rect.right != 214 || rect.bottom != 155) ExitProcess(22);
        if (mode == 2) return -1;
        if (!nesting) {
            nesting = 1;
            nested = create("nested");
            if (!nested || nested == window) ExitProcess(23);
        }
    }
    return DefWindowProcA(window, message, word, parameter);
}

void entry(void) {
    static const WindowClass wc = {3, procedure, 0, 0, (Handle)0x400000, 0, 0, 0, 0, className};
    Handle window, hookHandle;
    Rect rect;
    if (!RegisterClassA(&wc)) ExitProcess(1);
    hookHandle = SetWindowsHookExA(5, hook, 0, 1);
    if (!hookHandle) ExitProcess(2);
    window = create("outer");
    if (!window || !nested || count != 8 || hooks != 2) ExitProcess(3);
    if (GetActiveWindow()) ExitProcess(16);
    for (unsigned int i = 0; i < 8; i += 4) {
        if (messages[i] != 0x24 || messages[i+1] != 0x81 || messages[i+2] != 0x83 || messages[i+3] != 1) ExitProcess(4);
    }
    if (FindWindowA(className, "outer") != window || FindWindowA(className, "nested") != nested) ExitProcess(5);
    if (!GetWindowRect(window, &rect) || rect.left != 10 || rect.top != 20 || rect.right != 230 || rect.bottom != 200) ExitProcess(6);
    for (mode = 1; mode <= 3; mode++) {
        count = 0;
        if (create("rejected") || GetLastError() != 77 || FindWindowA(className, "rejected")) ExitProcess(7);
        if (GetActiveWindow()) ExitProcess(17);
        if (mode == 1 && (count != 3 || messages[2] != 0x82)) ExitProcess(8);
        if (mode == 2 && (count != 6 || messages[4] != 2 || messages[5] != 0x82)) ExitProcess(9);
        if (mode == 3 && count) ExitProcess(14);
    }
    if (ShowWindow(window, 1) || GetActiveWindow() != window ||
        !(GetWindowLongA(window, -16) & 0x10000000) || !ShowWindow(window, 1)) ExitProcess(24);
    if (ShowWindow(0, 1) || GetLastError() != 1400) ExitProcess(25);
    if (UpdateWindow(0) || GetLastError() != 1400) ExitProcess(26);
    SetLastError(77);
    Handle visible = CreateWindowExA(0, className, "visible", 0x10ca0000, 10, 20, 130, 90, 0, 0, (Handle)0x400000, (void *)1234);
    if (!visible || GetActiveWindow() != visible || FindWindowA(className, "visible") != visible) ExitProcess(18);
    if (!IsWindow(window) || !IsWindow(nested) || !UnhookWindowsHookEx(hookHandle)) ExitProcess(15);
    ExitProcess(42);
}
