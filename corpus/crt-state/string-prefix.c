__declspec(dllimport) int __cdecl strncmp(const char *, const char *, unsigned int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const char a[] = {'a', 'b', 'c', 0, 'x'};
static const char b[] = {'a', 'b', 'c', 0, 'z'};
static const char high[] = {(char)0x80, 0};
static const char low[] = {0x7f, 0};

void entry(void) {
    int *error = _errno();
    *error = 88;
    SetLastError(77);
    if (strncmp("abcD", "abcZ", 3)) ExitProcess(1);
    if (strncmp("abcD", "abcZ", 4) >= 0) ExitProcess(2);
    if (strncmp(a, b, 5) || strncmp(0, 0, 0)) ExitProcess(3);
    if (strncmp(high, low, 2) <= 0 || strncmp(low, high, 2) >= 0) ExitProcess(4);
    if (strncmp("aaa", "aaa" + 1, 3) <= 0) ExitProcess(5);
    if (*error != 88 || GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
