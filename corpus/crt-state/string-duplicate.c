__declspec(dllimport) unsigned char *__cdecl _strdup(const unsigned char *);
__declspec(dllimport) void __cdecl free(void *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const unsigned char source[] = {'a', 0x80, 0xff, 0, 'z'};

void entry(void) {
    *_errno() = 123;
    SetLastError(77);
    unsigned char *first = _strdup(source);
    if (!first || first == source || first[0] != 'a' || first[1] != 0x80 || first[2] != 0xff || first[3]) ExitProcess(1);
    unsigned char *second = _strdup(first);
    if (!second || second == first) ExitProcess(2);
    first[0] = 'b';
    if (source[0] != 'a' || second[0] != 'a' || first[0] != 'b') ExitProcess(3);
    unsigned char *empty = _strdup(source + 3);
    if (!empty || empty[0]) ExitProcess(4);
    if (_strdup(0)) ExitProcess(5);
    free(first);
    free(second);
    free(empty);
    if (*_errno() != 123 || GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
