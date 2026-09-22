__declspec(dllimport) int __cdecl remove(const char *);
__declspec(dllimport) int __cdecl _stat(const char *, void *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned char info[36];
    int existed = _stat("folder\\erase.tmp", info) == 0;
    *_errno() = 77;
    int result = remove("folder\\erase.tmp");
    if (existed) {
        if (result != 0 || *_errno() != 77) ExitProcess(1);
    } else if (result != -1 || *_errno() != 2) ExitProcess(2);
    if (_stat("folder\\erase.tmp", info) != -1 || *_errno() != 2) ExitProcess(3);
    if (remove("folder\\erase.tmp") != -1 || *_errno() != 2) ExitProcess(4);
    if (remove(".") != -1 || *_errno() != 13) ExitProcess(5);
    if (existed && _stat("folder", info) != 0) ExitProcess(6);
    ExitProcess(42);
}
