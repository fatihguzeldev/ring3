__declspec(dllimport) int __cdecl _stricmp(const char *, const char *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const char high_a[] = {(char)0xc4, 0};
static const char high_b[] = {(char)0xe4, 0};

void entry(void) {
    int *error = _errno();
    *error = 88;
    SetLastError(77);
    if (_stricmp("MiXeD", "mixed") || _stricmp("", "")) ExitProcess(1);
    if (_stricmp("A", "_") <= 0 || _stricmp("JOHNSTON", "JOHN_HENRY") <= 0) ExitProcess(2);
    if (_stricmp(high_a, high_b) >= 0 || _stricmp(high_b, high_a) <= 0) ExitProcess(3);
    if (_stricmp("a", "AB") >= 0 || _stricmp("AB", "a") <= 0) ExitProcess(4);
    if (*error != 88 || GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
