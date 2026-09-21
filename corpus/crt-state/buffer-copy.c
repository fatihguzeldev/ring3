__declspec(dllimport) void *__cdecl memcpy(void *, const void *, unsigned int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char source[65537];
static unsigned char destination[65539];

void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    source[0] = 0x80;
    source[4096] = 0xff;
    source[65536] = 'a';
    destination[0] = 0x55;
    destination[65538] = 0x66;
    if (memcpy(destination + 1, source, sizeof(source)) != destination + 1)
        ExitProcess(1);
    if (destination[1] != 0x80 || destination[4096] != 0 ||
        destination[4097] != 0xff || destination[65537] != 'a') ExitProcess(2);
    if (destination[0] != 0x55 || destination[65538] != 0x66) ExitProcess(3);
    if (memcpy((void *)0xffffffff, 0, 0) != (void *)0xffffffff) ExitProcess(4);
    if (*error != 123 || GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
