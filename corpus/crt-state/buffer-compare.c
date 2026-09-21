typedef unsigned long DWORD;
__declspec(dllimport) int __cdecl memcmp(const void *, const void *, unsigned int);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(DWORD);
__declspec(dllimport) DWORD __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char a[600];
static unsigned char b[600];

void entry(void) {
    SetLastError(77);
    *_errno() = 88;
    a[257] = 0x80;
    b[257] = 0x7f;
    if (memcmp(a, b, 257)) ExitProcess(1);
    if (memcmp(a, b, 258) <= 0 || memcmp(b, a, 600) >= 0) ExitProcess(2);
    if (memcmp(a, a, 600) || memcmp(a, a + 1, 128)) ExitProcess(3);
    if (memcmp(0, 0, 0)) ExitProcess(4);
    b[257] = 0x80;
    if (memcmp(a, b, 600)) ExitProcess(5);
    if (GetLastError() != 77 || *_errno() != 88) ExitProcess(6);
    ExitProcess(42);
}
