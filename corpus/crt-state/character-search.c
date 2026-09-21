__declspec(dllimport) char *__cdecl strchr(const char *, int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char text[] = {'a', 0x80, 'a', 'b', 0, 'z'};

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    if (strchr((char *)text, 'a') != (char *)text) ExitProcess(1);
    if (strchr((char *)text, 0x123480) != (char *)text + 1) ExitProcess(2);
    if (strchr((char *)text, -128) != (char *)text + 1) ExitProcess(3);
    if (strchr((char *)text, 'z') != 0) ExitProcess(4);
    if (strchr((char *)text, 0) != (char *)text + 4) ExitProcess(5);
    if (strchr((char *)text + 4, 0) != (char *)text + 4) ExitProcess(6);
    if (strchr((char *)text + 4, 'a') != 0) ExitProcess(7);
    if (text[0] != 'a' || text[1] != 0x80 || text[4] != 0 || text[5] != 'z') ExitProcess(8);
    if (*error != 123 || GetLastError() != 77) ExitProcess(9);
    ExitProcess(42);
}
