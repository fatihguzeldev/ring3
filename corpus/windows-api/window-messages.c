typedef void *Handle;
typedef long (__stdcall *WindowProc)(Handle, unsigned int, unsigned int, long);
typedef struct { unsigned int style; WindowProc procedure; int classExtra, windowExtra; Handle instance, icon, cursor, background; const char *menu, *name; } WindowClass;
__declspec(dllimport) unsigned short __stdcall RegisterClassA(const WindowClass *);
__declspec(dllimport) Handle __stdcall CreateWindowExA(unsigned long, const char *, const char *, unsigned long, int, int, int, int, Handle, Handle, Handle, void *);
__declspec(dllimport) long __stdcall DefWindowProcA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) long __stdcall CallWindowProcA(WindowProc, Handle, unsigned int, unsigned int, long);
__declspec(dllimport) long __stdcall SetWindowLongA(Handle, int, long);
__declspec(dllimport) long __stdcall SendMessageA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) Handle __stdcall LoadIconA(Handle, const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static WindowProc previous;
static unsigned int consumeIcon, created;
static const char className[] = "message-test";

static long __stdcall replacement(Handle window, unsigned int message, unsigned int word, long parameter) {
    return CallWindowProcA(previous, window, message, word, parameter) + 1;
}

static long __stdcall procedure(Handle window, unsigned int message, unsigned int word, long parameter) {
    if (message == 0x400) return (long)word + parameter;
    if (message == 0x401) return SendMessageA(window, 0x400, word, parameter) + 7;
    if (message == 0x402) {
        previous = (WindowProc)SetWindowLongA(window, -4, (long)replacement);
        if (previous != procedure) ExitProcess(20);
        return SendMessageA(window, 0x400, word, parameter);
    }
    if (message == 0x81) {
        if (SendMessageA(window, 0x400, 10, 32) != 42) ExitProcess(21);
        created++;
    }
    if (consumeIcon && message == 0x80) return 123;
    return DefWindowProcA(window, message, word, parameter);
}

static Handle create(void) {
    return CreateWindowExA(0, className, "messages", 0x00ca0000, 0, 0, 200, 100, 0, 0, (Handle)0x400000, 0);
}

void entry(void) {
    static const WindowClass wc = {3, procedure, 0, 0, (Handle)0x400000, 0, 0, 0, 0, className};
    Handle first, second, icon;
    SetLastError(77);
    if (!RegisterClassA(&wc)) ExitProcess(1);
    first = create(); second = create();
    if (!first || !second || first == second || created != 2) ExitProcess(2);
    icon = LoadIconA((Handle)0x400000, (const char *)7);
    if (!icon) ExitProcess(3);
    consumeIcon = 1;
    if (SendMessageA(first, 0x80, 1, (long)icon) != 123 || SendMessageA(first, 0x7f, 1, 0)) ExitProcess(4);
    consumeIcon = 0;
    if (SendMessageA(first, 0x80, 1, (long)icon) || SendMessageA(first, 0x7f, 1, 0) != (long)icon) ExitProcess(5);
    if (SendMessageA(first, 0x80, 1, (long)icon) != (long)icon) ExitProcess(6);
    if (SendMessageA(first, 0x7f, 0, 0) || SendMessageA(second, 0x7f, 1, 0)) ExitProcess(7);
    if (SendMessageA(first, 0x80, 0, (long)icon) || SendMessageA(first, 0x7f, 0, 0) != (long)icon) ExitProcess(8);
    if (SendMessageA(first, 0x80, 1, 0) != (long)icon || SendMessageA(first, 0x7f, 1, 0)) ExitProcess(9);
    if (SendMessageA(first, 0x401, 10, 32) != 49) ExitProcess(10);
    if (SendMessageA(first, 0x402, 10, 32) != 43 || SendMessageA(first, 0x400, 10, 32) != 43) ExitProcess(11);
    if (SendMessageA(second, 0x400, 10, 32) != 42) ExitProcess(12);
    if (SetWindowLongA(first, -4, (long)previous) != (long)replacement) ExitProcess(13);
    if (SendMessageA(first, 0x400, 10, 32) != 42 || GetLastError() != 77) ExitProcess(14);
    ExitProcess(42);
}
