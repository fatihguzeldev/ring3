__declspec(dllimport) unsigned char *__cdecl _mbsinc(const unsigned char *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) unsigned char *__stdcall lstrcpynA(unsigned char *, const unsigned char *, int);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const unsigned char path[] = "group\\file.ext";
static const unsigned char high[] = {0x80, 0xff, 0};
static unsigned char output[10];

void entry(void) {
    const unsigned char *cursor = path;
    const unsigned char *part = path;
    *_errno() = 123;
    SetLastError(77);
    while (*cursor) {
        if (*cursor == '\\') part = _mbsinc(cursor);
        cursor = _mbsinc(cursor);
    }
    if (part != path + 6 || cursor != path + 14) ExitProcess(1);
    if (_mbsinc(high) != high + 1 || _mbsinc(high + 2) != high + 3) ExitProcess(2);
    output[8] = 0x55;
    if (lstrcpynA(output, part, 8) != output) ExitProcess(3);
    if (output[0] != 'f' || output[4] != '.' || output[6] != 'x' || output[7] || output[8] != 0x55) ExitProcess(4);
    if (lstrcpynA(output, high, -1) != output || output[0] != 0x80 || output[1] != 0xff || output[2]) ExitProcess(5);
    if (lstrcpynA(output, 0, 1) != output || output[0]) ExitProcess(6);
    if (lstrcpynA(output, 0, 0) != output) ExitProcess(7);
    if (*_errno() != 123 || GetLastError() != 77) ExitProcess(8);
    ExitProcess(42);
}
