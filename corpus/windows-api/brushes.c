typedef struct { unsigned int style, color, hatch; } Brush;
__declspec(dllimport) void *__stdcall GetSysColorBrush(int);
__declspec(dllimport) unsigned long __stdcall GetSysColor(int);
__declspec(dllimport) int __stdcall GetObjectA(void *, int, void *);
__declspec(dllimport) int __stdcall DeleteObject(void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(77);
    void *first = GetSysColorBrush(15);
    void *other = GetSysColorBrush(1);
    if (!first || !other || first == other) ExitProcess(1);
    if (GetSysColorBrush(15) != first) ExitProcess(2);
    if (GetObjectA(first, 0, 0) != sizeof(Brush)) ExitProcess(3);
    Brush brush;
    if (GetObjectA(first, sizeof(brush), &brush) != sizeof(brush)) ExitProcess(4);
    if (brush.style || brush.hatch || brush.color != GetSysColor(15)) ExitProcess(5);
    if (!DeleteObject(first) || GetSysColorBrush(15) != first) ExitProcess(6);
    if (GetObjectA(first, sizeof(brush), &brush) != sizeof(brush)) ExitProcess(7);
    brush.color = 0xdeadbeef;
    if (GetObjectA(other, 4, &brush) != 4 || brush.color != 0xdeadbeef) ExitProcess(8);
    if (GetSysColorBrush(-1) || GetSysColorBrush(31) || DeleteObject(0)) ExitProcess(9);
    if (GetObjectA(0, sizeof(brush), &brush)) ExitProcess(10);
    if (GetLastError() != 77) ExitProcess(11);
    ExitProcess(42);
}
