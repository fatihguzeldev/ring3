__declspec(dllimport) unsigned char *__cdecl _mbsrchr(const unsigned char *, unsigned int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const unsigned char path[] = "folder.name\\module.ext";
static const unsigned char high[] = {0x80, 0xff, 0x80, 0};

void entry(void) {
    *_errno() = 123;
    SetLastError(77);
    if (_mbsrchr(path, '.') != path + 18) ExitProcess(1);
    if (_mbsrchr(path, 0) != path + 22) ExitProcess(2);
    if (_mbsrchr(path, '/') != 0) ExitProcess(3);
    if (_mbsrchr(path + 22, '.') != 0) ExitProcess(4);
    if (_mbsrchr(high, 0x180) != high + 2) ExitProcess(5);
    if (_mbsrchr(high, 0xffffffff) != high + 1) ExitProcess(6);
    if (_mbsrchr(high, 0x100) != high + 3) ExitProcess(7);
    if (*_errno() != 123 || GetLastError() != 77) ExitProcess(8);
    ExitProcess(42);
}
