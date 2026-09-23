typedef void *Handle;
typedef int (__stdcall *DialogProc)(Handle, unsigned int, unsigned int, long);
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
    Control label;
} Template;
#pragma pack(pop)

__declspec(dllimport) Handle __stdcall CreateDialogIndirectParamA(Handle, const void *, Handle, DialogProc, long);
__declspec(dllimport) Handle __stdcall GetDlgItem(Handle, int);
__declspec(dllimport) Handle __stdcall GetTopWindow(Handle);
__declspec(dllimport) Handle __stdcall GetWindow(Handle, unsigned int);
__declspec(dllimport) int __stdcall SetWindowTextA(Handle, const char *);
__declspec(dllimport) Handle __stdcall FindWindowA(const char *, const char *);
__declspec(dllimport) long __stdcall SendMessageA(Handle, unsigned int, unsigned int, long);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) Handle __stdcall GetParent(Handle);
__declspec(dllimport) Handle __stdcall GetActiveWindow(void);
__declspec(dllimport) int __stdcall IsWindow(Handle);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const Template dialog = {
    1, 0xffff, 0, 0, 0x80c00000, 2, 0, 0, 100, 50, 0, 0, {'T', 0}, 0,
    {0, 0, 0x50010000, 4, 5, 30, 12, 42, 0xffff, 0x80, {'O', 'K', 0}, 0},
    {0, 0, 0x50000000, 40, 5, 30, 12, 43, 0xffff, 0x82, {'H', 'i', 0}, 0}
};
static int callbacks;

static int __stdcall procedure(Handle window, unsigned int message, unsigned int focus, long init) {
    if (message != 0x110) ExitProcess(10);
    if (init != 123 || !IsWindow(window) || !IsWindow((Handle)focus)) ExitProcess(11);
    if (GetDlgItem(window, 42) != (Handle)focus || GetParent((Handle)focus) != window) ExitProcess(12);
    if (!IsWindow(GetDlgItem(window, 43))) ExitProcess(13);
    callbacks++;
    return 1;
}

void entry(void) {
    Handle window = CreateDialogIndirectParamA((Handle)0x400000, &dialog, 0, procedure, 123);
    if (!window || callbacks != 1 || !GetDlgItem(window, 42) || GetActiveWindow()) ExitProcess(1);
    if (GetTopWindow(window) != GetDlgItem(window, 43) ||
        GetTopWindow(GetDlgItem(window, 42)) || GetTopWindow(0) != window)
        ExitProcess(2);
    if (GetWindow(GetDlgItem(window, 43), 2) != GetDlgItem(window, 42) ||
        GetWindow(GetDlgItem(window, 42), 3) != GetDlgItem(window, 43) ||
        GetWindow(GetDlgItem(window, 43), 0) != GetDlgItem(window, 43) ||
        GetWindow(GetDlgItem(window, 42), 1) != GetDlgItem(window, 42) ||
        GetWindow(window, 5) != GetDlgItem(window, 43) ||
        GetWindow(GetDlgItem(window, 42), 2)) ExitProcess(4);
    if (!SetWindowTextA(window, "renamed") || FindWindowA(0, "renamed") != window)
        ExitProcess(5);
    SetLastError(77);
    if (SendMessageA(GetDlgItem(window, 42), 0x364, 0, 0) ||
        SendMessageA(GetDlgItem(window, 43), 0x364, 0, 0) || GetLastError() != 77)
        ExitProcess(3);
    ExitProcess(42);
}
