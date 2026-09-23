typedef void *Handle;
typedef int (__stdcall *DialogProc)(Handle, unsigned int, unsigned int, long);
typedef long (__stdcall *HookProc)(int, unsigned int, long);
typedef struct {
    void *parameters;
    Handle instance, menu, parent;
    int height, width, y, x;
    unsigned long style;
    const char *title, *className;
    unsigned long exstyle;
} CreateStruct;
typedef struct { CreateStruct *creation; Handle insertAfter; } CbtCreate;
typedef struct {
    unsigned long help, exstyle, style;
    short x, y, width, height;
    unsigned long id;
    unsigned short classTag, classOrdinal, title[3], extra;
} Control;
#pragma pack(push, 1)
typedef struct {
    unsigned short version, signature;
    unsigned long help, exstyle, style;
    unsigned short count;
    short x, y, width, height;
    unsigned short menu, windowClass, title[2], padding;
    Control button;
} Template;
#pragma pack(pop)

__declspec(dllimport) Handle __stdcall SetWindowsHookExA(int, HookProc, Handle, unsigned long);
__declspec(dllimport) int __stdcall UnhookWindowsHookEx(Handle);
__declspec(dllimport) long __stdcall CallNextHookEx(Handle, int, unsigned int, long);
__declspec(dllimport) Handle __stdcall CreateDialogIndirectParamA(Handle, const void *, Handle, DialogProc, long);
__declspec(dllimport) Handle __stdcall GetDlgItem(Handle, int);
__declspec(dllimport) int __stdcall IsWindow(Handle);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const Template dialog = {
    1, 0xffff, 0, 0, 0x80c00000, 1, 0, 0, 100, 50, 0, 0, {'T', 0}, 0,
    {0, 0, 0x50010000, 4, 5, 30, 12, 42, 0xffff, 0x80, {'O', 'K', 0}, 0}
};
static Handle hookHandle, attached;
static int phase, veto;

static long __stdcall hook(int code, unsigned int window, long parameter) {
    CbtCreate *create = (CbtCreate *)parameter;
    if (code != 3 || !IsWindow((Handle)window) || !create || !create->creation ||
        create->creation->instance != (Handle)0x400000 ||
        create->creation->style != 0x80c00000 || create->insertAfter)
        ExitProcess(11);
    attached = (Handle)window;
    phase = 1;
    if (CallNextHookEx(hookHandle, code, window, parameter)) ExitProcess(12);
    return veto;
}

static int __stdcall procedure(Handle window, unsigned int message, unsigned int focus, long init) {
    if (phase != 1 || attached != window || message != 0x110 || init != 123 ||
        GetDlgItem(window, 42) != (Handle)focus)
        ExitProcess(13);
    phase = 2;
    return 1;
}

void entry(void) {
    hookHandle = SetWindowsHookExA(5, hook, 0, 1);
    if (!hookHandle) ExitProcess(1);
    Handle window = CreateDialogIndirectParamA((Handle)0x400000, &dialog, 0, procedure, 123);
    if (!window || window != attached || phase != 2) ExitProcess(2);
    veto = 1;
    phase = 0;
    if (CreateDialogIndirectParamA((Handle)0x400000, &dialog, 0, procedure, 123)) ExitProcess(3);
    if (phase != 1 || IsWindow(attached)) ExitProcess(4);
    if (!UnhookWindowsHookEx(hookHandle)) ExitProcess(5);
    ExitProcess(42);
}
