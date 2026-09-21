typedef struct {
    unsigned int dev;
    unsigned short ino, mode;
    short links, uid, gid;
    unsigned int rdev;
    int size, accessed, modified, created;
} LEGACY_STAT;
_Static_assert(sizeof(LEGACY_STAT) == 36, "legacy stat size");
_Static_assert(__builtin_offsetof(LEGACY_STAT, mode) == 6, "mode offset");
_Static_assert(__builtin_offsetof(LEGACY_STAT, size) == 20, "size offset");
_Static_assert(__builtin_offsetof(LEGACY_STAT, accessed) == 24, "time offset");
__declspec(dllimport) int __cdecl _stat(const char *, LEGACY_STAT *);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    LEGACY_STAT info;
    *_errno() = 77;
    SetLastError(99);
    if (_stat(".", &info) || info.dev != 2 || info.rdev != 2 || info.mode != 0x41ff ||
        info.ino || info.links != 1 || info.uid || info.gid || info.size ||
        info.accessed || info.modified || info.created) ExitProcess(1);
    if (*_errno() != 77 || GetLastError() != 99) ExitProcess(2);
    if (_stat("C:\\", &info) || _stat("\\", &info)) ExitProcess(3);
    info.size = 55;
    if (_stat("missing", &info) != -1 || *_errno() != 2 || info.size != 55) ExitProcess(4);
    if (_stat("*", &info) != -1 || *_errno() != 2 || GetLastError() != 99) ExitProcess(5);
    ExitProcess(42);
}
