typedef void *(__cdecl *Move)(void *, const void *, unsigned int);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void *__stdcall GetProcAddress(void *, const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char forward[10] = "abcdefghij";
static unsigned char backward[10] = "abcdefghij";
static unsigned char same[10] = "abcdefghij";

static int equals(const unsigned char *actual, const char *expected) {
    unsigned int index;
    for (index = 0; index < 10; index++) {
        if (actual[index] != (unsigned char)expected[index]) return 0;
    }
    return 1;
}

void entry(void) {
    Move move = (Move)GetProcAddress(GetModuleHandleA("MSVCRT.dll"), "memmove");
    if (!move) ExitProcess(1);
    SetLastError(77);
    if (move((void *)0xffffffff, 0, 0) != (void *)0xffffffff) ExitProcess(2);
    if (move(forward + 2, forward, 6) != forward + 2 ||
        !equals(forward, "ababcdefij")) ExitProcess(3);
    if (move(backward, backward + 2, 6) != backward ||
        !equals(backward, "cdefghghij")) ExitProcess(4);
    if (move(same, same, 10) != same || !equals(same, "abcdefghij")) ExitProcess(5);
    if (GetLastError() != 77) ExitProcess(6);
    ExitProcess(42);
}
