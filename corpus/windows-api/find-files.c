typedef unsigned long DWORD;
typedef struct { DWORD low, high; } FILETIME;
typedef struct {
    DWORD attributes;
    FILETIME created, accessed, written;
    DWORD size_high, size_low, reserved0, reserved1;
    char name[260], alternate[14];
} FIND_DATA;
_Static_assert(sizeof(FIND_DATA) == 320, "find data size");
_Static_assert(__builtin_offsetof(FIND_DATA, name) == 44, "filename offset");
_Static_assert(__builtin_offsetof(FIND_DATA, alternate) == 304, "alternate offset");
__declspec(dllimport) void *__stdcall FindFirstFileA(const char *, FIND_DATA *);
__declspec(dllimport) int __stdcall FindNextFileA(void *, FIND_DATA *);
__declspec(dllimport) int __stdcall FindClose(void *);
__declspec(dllimport) void __stdcall SetLastError(DWORD);
__declspec(dllimport) DWORD __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    FIND_DATA data;
    data.attributes = 0x55;
    SetLastError(77);
    void *handle = FindFirstFileA("*", &data);
    if (handle == (void *)-1) {
        if (GetLastError() != 2 || data.attributes != 0x55) ExitProcess(1);
    } else {
        if (GetLastError() != 77 || data.name[0] != 'a' || data.attributes != 0x80 ||
            data.size_high != 1 || data.size_low != 7 || data.alternate[0]) ExitProcess(2);
        if (!FindNextFileA(handle, &data) || data.name[0] != 'b' || data.size_low != 5) ExitProcess(3);
        if (!FindNextFileA(handle, &data) || data.name[0] != 'D' || data.attributes != 0x10) ExitProcess(4);
        if (FindNextFileA(handle, &data) || GetLastError() != 18) ExitProcess(5);
        if (!FindClose(handle)) ExitProcess(6);
        if (FindClose(handle) || GetLastError() != 6) ExitProcess(7);
    }
    if (FindFirstFileA("missing\\*", &data) != (void *)-1 || GetLastError() != 3) ExitProcess(8);
    if (FindNextFileA((void *)0, &data) || GetLastError() != 6) ExitProcess(9);
    if (FindClose((void *)0) || GetLastError() != 6) ExitProcess(10);
    ExitProcess(42);
}
