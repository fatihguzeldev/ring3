typedef long (__stdcall *WindowProc)(void *, unsigned int, unsigned int, long);
typedef struct {
    unsigned int style;
    WindowProc procedure;
    int class_extra, window_extra;
    void *instance, *icon, *cursor, *background;
    const char *menu, *name;
} WindowClass;
_Static_assert(sizeof(WindowClass) == 40, "guest32 class size");
_Static_assert(__builtin_offsetof(WindowClass, instance) == 16, "instance offset");
_Static_assert(__builtin_offsetof(WindowClass, name) == 36, "name offset");
__declspec(dllimport) unsigned short __stdcall RegisterClassA(const WindowClass *);
__declspec(dllimport) int __stdcall GetClassInfoA(void *, const char *, WindowClass *);
__declspec(dllimport) int __stdcall UnregisterClassA(const char *, void *);
__declspec(dllimport) unsigned int __stdcall RegisterWindowMessageA(const char *);
__declspec(dllimport) unsigned int __stdcall RegisterClipboardFormatA(const char *);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);
static long __stdcall procedure(void *window, unsigned int message, unsigned int word, long parameter) {
    (void)window; (void)message; (void)word; (void)parameter;
    return 0;
}
void entry(void) {
    static WindowClass value, output;
    value.style = 3;
    value.procedure = procedure;
    value.instance = GetModuleHandleA(0);
    value.name = "ExampleClass";
    unsigned int message = RegisterWindowMessageA("OtherName");
    if (GetClassInfoA(value.instance, value.name, &output) || GetLastError() != 1411) ExitProcess(1);
    SetLastError(77);
    unsigned int atom = RegisterClassA(&value);
    if (!atom || atom == message || GetLastError() != 77) ExitProcess(2);
    if (RegisterClipboardFormatA(value.name) != atom) ExitProcess(9);
    if (RegisterClassA(&value) || GetLastError() != 1410) ExitProcess(3);
    if (!GetClassInfoA(value.instance, "EXAMPLECLASS", &output)) ExitProcess(4);
    if (output.style != 3 || output.procedure != procedure || output.instance != value.instance) ExitProcess(5);
    if (!GetClassInfoA(value.instance, (const char *)atom, &output) || output.name != (const char *)atom) ExitProcess(6);
    if (!UnregisterClassA((const char *)atom, value.instance)) ExitProcess(7);
    if (GetClassInfoA(value.instance, value.name, &output) || GetLastError() != 1411) ExitProcess(8);
    if (RegisterClassA(&value) != atom || RegisterWindowMessageA(value.name) != atom) ExitProcess(10);
    ExitProcess(42);
}
