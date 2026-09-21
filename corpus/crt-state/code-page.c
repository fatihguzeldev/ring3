__declspec(dllimport) int __cdecl _setmbcp(int);
__declspec(dllimport) unsigned char *__cdecl _mbsrchr(const unsigned char *, unsigned int);
__declspec(dllimport) unsigned char *__cdecl _mbsinc(const unsigned char *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const int selectors[] = {-3, 1252, -2, 437, 0};
static const unsigned char text[] = {0x81, 0xfe, 0x81, 0};

void entry(void) {
    *_errno() = 123;
    SetLastError(77);
    for (int i = 0; i < 5; ++i) {
        if (_setmbcp(selectors[i])) ExitProcess(1);
        if (_mbsinc(text) != text + 1) ExitProcess(2);
        if (_mbsrchr(text, 0x181) != text + 2) ExitProcess(3);
    }
    if (*_errno() != 123 || GetLastError() != 77) ExitProcess(4);
    ExitProcess(42);
}
