typedef struct { int x, y; } Point;
__declspec(dllimport) int __stdcall GetCursorPos(Point *);
__declspec(dllimport) int __stdcall SetCursorPos(int, int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    Point point;
    SetLastError(77);
    if (!GetCursorPos(&point) || point.x || point.y) ExitProcess(1);
    if (!SetCursorPos(320, 240) || !GetCursorPos(&point)) ExitProcess(2);
    if (point.x != 320 || point.y != 240) ExitProcess(3);
    if (!SetCursorPos(-1, 480) || !GetCursorPos(&point)) ExitProcess(4);
    if (point.x != 0 || point.y != 479) ExitProcess(5);
    if (GetLastError() != 77) ExitProcess(6);
    if (GetCursorPos((Point *)0) || GetLastError() != 998) ExitProcess(7);
    ExitProcess(42);
}
