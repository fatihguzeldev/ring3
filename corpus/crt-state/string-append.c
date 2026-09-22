__declspec(dllimport) char *__cdecl strncat(char *, const char *, unsigned int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const unsigned char source[] = {'a', 0x80, 'b', 0};
static unsigned char destination[16] = {'Q', 0, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55};

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    if (strncat((char *)destination, (const char *)source, 2) != (char *)destination) ExitProcess(1);
    if (destination[0] != 'Q' || destination[1] != 'a' || destination[2] != 0x80 ||
        destination[3] != 0 || destination[4] != 0x55) ExitProcess(2);
    if (strncat((char *)destination, (const char *)source, 8) != (char *)destination) ExitProcess(3);
    if (destination[3] != 'a' || destination[4] != 0x80 || destination[5] != 'b' ||
        destination[6] != 0 || destination[7] != 0x55) ExitProcess(4);
    if (strncat((char *)destination, (const char *)source + 3, 1) != (char *)destination) ExitProcess(5);
    if (strncat((char *)destination, 0, 0) != (char *)destination) ExitProcess(6);
    if (destination[6] != 0 || destination[7] != 0x55 || *error != 123 || GetLastError() != 77) ExitProcess(7);
    ExitProcess(42);
}
