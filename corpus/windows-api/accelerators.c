typedef void *HANDLE;
typedef struct { unsigned char flags; unsigned short key, command; } ACCEL;
_Static_assert(sizeof(ACCEL) == 6, "ACCEL size");
_Static_assert(__builtin_offsetof(ACCEL, key) == 2, "ACCEL key offset");
_Static_assert(__builtin_offsetof(ACCEL, command) == 4, "ACCEL command offset");
__declspec(dllimport) HANDLE __stdcall LoadAcceleratorsA(HANDLE, const char *);
__declspec(dllimport) int __stdcall CopyAcceleratorTableA(HANDLE, ACCEL *, int);
__declspec(dllimport) HANDLE __stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static ACCEL output[4];

void entry(void) {
    HANDLE module = GetModuleHandleA(0);
    SetLastError(77);
    HANDLE table = LoadAcceleratorsA(0, (const char *)1);
    if (!table || LoadAcceleratorsA(module, (const char *)1) != table) ExitProcess(1);
    if (CopyAcceleratorTableA(table, 0, 0) != 3 || CopyAcceleratorTableA(table, 0, -1) != 3) ExitProcess(2);
    output[1].command = 0x5555;
    output[3].command = 0x5555;
    if (CopyAcceleratorTableA(table, output, 1) != 1 || output[1].command != 0x5555) ExitProcess(3);
    if (output[0].flags != 9 || output[0].key != 'A' || output[0].command != 100) ExitProcess(4);
    if (CopyAcceleratorTableA(table, output, 99) != 3 || output[3].command != 0x5555) ExitProcess(5);
    if (output[1].flags || output[1].key != 'x' || output[1].command != 200) ExitProcess(6);
    if (output[2].flags != 9 || output[2].key != 'A' || output[2].command != 100) ExitProcess(7);
    if (((unsigned char *)output)[1] || GetLastError() != 77) ExitProcess(8);
    if (LoadAcceleratorsA(module, (const char *)2) || GetLastError() != 1814) ExitProcess(9);
    ExitProcess(42);
}
