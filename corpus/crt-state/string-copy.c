__declspec(dllimport) char *__cdecl strncpy(char *, const char *, unsigned int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char source[] = {'a', 0x80, 'b', 0};
static unsigned char destination[65539];

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    destination[0] = 0x55;
    destination[65538] = 0x66;
    destination[65537] = 0x77;
    if (strncpy((char *)destination + 1, (char *)source, 65537) != (char *)destination + 1)
        ExitProcess(1);
    if (destination[1] != 'a' || destination[2] != 0x80 || destination[3] != 'b' ||
        destination[4] != 0 || destination[4097] != 0 || destination[65537] != 0)
        ExitProcess(2);
    if (destination[0] != 0x55 || destination[65538] != 0x66) ExitProcess(3);
    destination[4] = 0x77;
    if (strncpy((char *)destination + 1, (char *)source, 3) != (char *)destination + 1 ||
        destination[3] != 'b' || destination[4] != 0x77) ExitProcess(4);
    if (strncpy((char *)destination + 1, (char *)source + 3, 3) != (char *)destination + 1 ||
        destination[1] != 0 || destination[3] != 0 || destination[4] != 0x77) ExitProcess(5);
    if (strncpy((char *)0xffffffff, 0, 0) != (char *)0xffffffff) ExitProcess(6);
    if (*error != 123 || GetLastError() != 77) ExitProcess(7);
    ExitProcess(42);
}
