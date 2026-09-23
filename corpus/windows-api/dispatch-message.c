typedef void *Handle;
typedef long (__stdcall *WindowProc)(Handle, unsigned int, unsigned int, long);
typedef struct { unsigned int style; WindowProc procedure; int classExtra, windowExtra; Handle instance, icon, cursor, background; const char *menu, *name; } WindowClass;
__declspec(dllimport) unsigned short __stdcall RegisterClassA(const WindowClass *);
__declspec(dllimport) Handle __stdcall CreateWindowExA(unsigned long, const char *, const char *, unsigned long, int, int, int, int, Handle, Handle, Handle, void *);
__declspec(dllimport) long __stdcall DefWindowProcA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) int __stdcall GetMessageA(void *, void *, unsigned int, unsigned int);
__declspec(dllimport) long __stdcall DispatchMessageA(const void *);
__declspec(dllimport) int __stdcall PostMessageA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const char name[] = "dispatch-test";
static long __stdcall procedure(Handle window, unsigned int message, unsigned int word, long parameter) {
    if (message == 0x401) return (long)word + parameter;
    return DefWindowProcA(window, message, word, parameter);
}

__declspec(noreturn) void entry(void) {
    static const WindowClass wc = {3, procedure, 0, 0, (Handle)0x400000, 0, 0, 0, 0, name};
    unsigned int message[8];
    SetLastError(77);
    if (!RegisterClassA(&wc)) ExitProcess(1);
    Handle window = CreateWindowExA(0, name, "dispatch", 0x00ca0000, 0, 0, 200, 100,
                                    0, 0, (Handle)0x400000, 0);
    if (!window) ExitProcess(2);
    if (GetMessageA(message, 0, 0, 0) != 1 || message[0] != (unsigned int)window ||
        message[1] != 0x401 || message[2] != 10 || message[3] != 32) ExitProcess(3);
    if (DispatchMessageA(message) != 42 || message[1] != 0x401 || GetLastError() != 77)
        ExitProcess(4);
    if (!PostMessageA(window, 0x402, 11, 31) ||
        GetMessageA(message, 0, 0, 0) != 1 || message[0] != (unsigned int)window ||
        message[1] != 0x402 || message[2] != 11 || message[3] != 31 ||
        GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
