typedef int (__cdecl *Callback)(void);
__declspec(dllimport) Callback __cdecl _onexit(Callback);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned int trace;
static int first(void) { trace = trace * 10 + 1; return 0; }
static int second(void) { trace = trace * 10 + 2; return 0; }

void entry(void) {
    *_errno() = 123;
    SetLastError(77);
    if (_onexit(first) != first || _onexit(second) != second || _onexit(first) != first) ExitProcess(1);
    if (_onexit(0) || trace) ExitProcess(2);
    if (*_errno() != 123 || GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
