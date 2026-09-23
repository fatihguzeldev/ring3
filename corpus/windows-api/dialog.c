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
__declspec(dllimport) int __stdcall EnableWindow(Handle, int);
__declspec(dllimport) long __stdcall GetWindowLongA(Handle, int);
__declspec(dllimport) int __stdcall ShowWindow(Handle, int);
__declspec(dllimport) int __stdcall UpdateWindow(Handle);
__declspec(dllimport) int __stdcall EndDialog(Handle, int);
__declspec(dllimport) int __stdcall SetWindowPos(Handle, Handle, int, int, int, int, unsigned int);
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
    {0, 0, 0x50000103, 40, 5, 30, 12, 43, 0xffff, 0x85, {'H', 'i', 0}, 0}
};
static int callbacks;
static int paints;

static int __stdcall procedure(Handle window, unsigned int message, unsigned int focus, long init) {
    if (message == 0x0f) { paints++; return 0; }
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
    Handle combo = GetDlgItem(window, 43);
    if (SendMessageA(combo, 0x146, 0, 0) != 0 ||
        SendMessageA(combo, 0x143, 0, (long)"pear") != 0 ||
        SendMessageA(combo, 0x151, 0, 0x1234) != 0 ||
        SendMessageA(combo, 0x143, 0, (long)"apple") != 0 ||
        SendMessageA(combo, 0x143, 0, (long)"orange") != 1 ||
        SendMessageA(combo, 0x146, 0, 0) != 3 ||
        SendMessageA(combo, 0x150, 2, 0) != 0x1234 ||
        SendMessageA(combo, 0x151, 1, 0x5678) != 0 ||
        SendMessageA(combo, 0x150, 1, 0) != 0x5678 ||
        SendMessageA(combo, 0x150, 0, 0) != 0 ||
        SendMessageA(combo, 0x151, 4, 99) != -1 ||
        SendMessageA(combo, 0x150, 4, 0) != -1) ExitProcess(6);
    if (SendMessageA(combo, 0x147, 0, 0) != -1 ||
        SendMessageA(combo, 0x14e, 2, 0) != 2 ||
        SendMessageA(combo, 0x147, 0, 0) != 2 ||
        SendMessageA(combo, 0x14e, -1, 0) != -1 ||
        SendMessageA(combo, 0x147, 0, 0) != -1 ||
        SendMessageA(combo, 0x14e, 5, 0) != -1 ||
        SendMessageA(combo, 0x14e, 0, 0) != 0 ||
        SendMessageA(combo, 0x147, 0, 0) != 0) ExitProcess(7);
    if (SendMessageA(combo, 0x14b, 0, 0) != 0 ||
        SendMessageA(combo, 0x146, 0, 0) != 0 ||
        SendMessageA(combo, 0x147, 0, 0) != -1 ||
        SendMessageA(combo, 0x150, 0, 0) != -1 ||
        SendMessageA(combo, 0x143, 0, (long)"new") != 0 ||
        SendMessageA(combo, 0x146, 0, 0) != 1) ExitProcess(8);
    Handle button = GetDlgItem(window, 42);
    if (GetWindowLongA(button, -16) & 0x08000000 ||
        EnableWindow(button, 0) != 0 ||
        !(GetWindowLongA(button, -16) & 0x08000000) ||
        EnableWindow(button, 0) != 1 ||
        EnableWindow(button, 1) != 1 ||
        GetWindowLongA(button, -16) & 0x08000000 ||
        EnableWindow(button, 1) != 0) ExitProcess(9);
    SetLastError(77);
    if (SendMessageA(GetDlgItem(window, 42), 0x364, 0, 0) ||
        SendMessageA(GetDlgItem(window, 43), 0x364, 0, 0) || GetLastError() != 77)
        ExitProcess(3);
    if (ShowWindow(window, 1) || !UpdateWindow(window) || paints != 1 ||
        !UpdateWindow(window) || paints != 1) ExitProcess(14);
    if (!EndDialog(window, 123) ||
        GetWindowLongA(window, -16) & 0x10000000 ||
        GetActiveWindow() || !IsWindow(window) || !IsWindow(button)) ExitProcess(15);
    if (!SetWindowPos(window, 0, 0, 0, 0, 0, 0x97) ||
        GetWindowLongA(window, -16) & 0x10000000 || GetActiveWindow() ||
        !IsWindow(window)) ExitProcess(16);
    ExitProcess(42);
}
