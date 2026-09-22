typedef unsigned long DWORD;
typedef void *HANDLE;
__declspec(dllimport) HANDLE __stdcall FindResourceA(HANDLE, const char *, const char *);
__declspec(dllimport) HANDLE __stdcall LoadResource(HANDLE, HANDLE);
__declspec(dllimport) const void * __stdcall LockResource(HANDLE);
__declspec(dllimport) DWORD __stdcall SizeofResource(HANDLE, HANDLE);
__declspec(dllimport) int __stdcall LoadStringA(HANDLE, unsigned int, unsigned char *, int);
__declspec(dllimport) HANDLE __stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void __stdcall SetLastError(DWORD);
__declspec(dllimport) DWORD __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char output[16];

void entry(void) {
    HANDLE module = GetModuleHandleA(0);
    SetLastError(77);
    HANDLE resource = FindResourceA(0, (const char *)1, (const char *)6);
    if (!resource || FindResourceA(module, (const char *)1, (const char *)6) != resource) ExitProcess(1);
    if (SizeofResource(module, resource) != 50) ExitProcess(9);
    HANDLE loaded = LoadResource(0, resource);
    if (!loaded || loaded == resource || LoadResource(module, resource) != loaded) ExitProcess(10);
    const unsigned short *data = (const unsigned short *)LockResource(loaded);
    if (!data || data[0] != 5 || data[1] != 'A' || data[6] != 4 || data[7] != 'B') ExitProcess(11);
    output[6] = 0x55;
    if (LoadStringA(0, 0, output, 6) != 5 || output[0] != 'A' || output[4] != 'a' || output[5] || output[6] != 0x55) ExitProcess(2);
    if (LoadStringA(module, 0x10000, output, 4) != 3 || output[2] != 'p' || output[3]) ExitProcess(3);
    if (LoadStringA(module, 1, output, 16) != 4 || output[0] != 'B' || output[3] != 'a' || output[4]) ExitProcess(4);
    if (LoadStringA(module, 15, output, 1) || output[0]) ExitProcess(5);
    if (GetLastError() != 77) ExitProcess(6);
    if (FindResourceA(module, (const char *)2, (const char *)6) || GetLastError() != 1814) ExitProcess(7);
    output[0] = 'x';
    if (LoadStringA(module, 16, output, 16) || output[0] || GetLastError() != 1814) ExitProcess(8);
    ExitProcess(42);
}
