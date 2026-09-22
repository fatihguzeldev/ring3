typedef struct {
    unsigned long cb;
    char *reserved, *desktop, *title;
    unsigned long x, y, x_size, y_size, x_chars, y_chars, fill, flags;
    unsigned short show, reserved_count;
    unsigned char *reserved_bytes;
    void *input, *output, *error;
} StartupInfo;

_Static_assert(sizeof(StartupInfo) == 68, "guest32 startup size");
_Static_assert(__builtin_offsetof(StartupInfo, flags) == 44, "flags offset");
_Static_assert(__builtin_offsetof(StartupInfo, show) == 48, "show offset");
_Static_assert(__builtin_offsetof(StartupInfo, reserved_bytes) == 52, "reserved offset");
_Static_assert(__builtin_offsetof(StartupInfo, error) == 64, "last handle offset");

__declspec(dllimport) void __stdcall GetStartupInfoA(StartupInfo *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    struct { unsigned int before; StartupInfo info; unsigned int after; } value;
    unsigned char *bytes = (unsigned char *)&value;
    for (unsigned int i = 0; i < sizeof(value); ++i) bytes[i] = 0xa5;
    SetLastError(77);
    GetStartupInfoA(&value.info);
    if (value.info.cb != 68 || value.before != 0xa5a5a5a5 || value.after != 0xa5a5a5a5) ExitProcess(1);
    bytes = (unsigned char *)&value.info;
    for (unsigned int i = 4; i < sizeof(StartupInfo); ++i) if (bytes[i]) ExitProcess(2);
    value.info.flags = 0xffffffff;
    GetStartupInfoA(&value.info);
    if (value.info.flags || GetLastError() != 77) ExitProcess(3);
    ExitProcess(42);
}
