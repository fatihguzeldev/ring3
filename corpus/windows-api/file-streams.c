typedef struct { char *ptr; int count; char *base; int flag, file, character, size; char *name; } File;
typedef File *(__cdecl *Open)(const char *, const char *);
typedef unsigned (__cdecl *Read)(void *, unsigned, unsigned, File *);
typedef int (__cdecl *Close)(File *);
typedef int (__cdecl *Remove)(const char *);
typedef int *(__cdecl *Errno)(void);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void *__stdcall GetProcAddress(void *, const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char output[16];
static const unsigned char expected[] = {'a', 'b', 0, 13, 10, 26, 'z', 255, 'E', 'N', 'D'};

void entry(void) {
    void *crt = GetModuleHandleA("MSVCRT.dll");
    Open open = (Open)GetProcAddress(crt, "fopen");
    Read read = (Read)GetProcAddress(crt, "fread");
    Close close = (Close)GetProcAddress(crt, "fclose");
    Remove remove = (Remove)GetProcAddress(crt, "remove");
    Errno error = (Errno)GetProcAddress(crt, "_errno");
    if (!open || !read || !close || !remove || !error) ExitProcess(1);
    int *errno_cell = error();
    *errno_cell = 123;
    SetLastError(77);
    File *first = open("./SAMPLE.bin", "rb");
    File *second = open("c:/sample.bin", "rb");
    if (!first || !second || first == second) ExitProcess(2);
    if (first->flag != 5 || first->file != 3 || second->file != 4) ExitProcess(3);
    if (read((void *)0xffffffff, 0, 0xffffffff, (File *)0xffffffff)) ExitProcess(4);
    if (read(output, 1, 11, first) != 11 || first->flag != 5) ExitProcess(5);
    if (read(output, 1, 1, first) || first->flag != 0x15) ExitProcess(6);
    for (unsigned i = 0; i < 16; i++) output[i] = 0x55;
    if (read(output, 2, 6, second) != 5 || second->flag != 0x15) ExitProcess(7);
    for (unsigned i = 0; i < 11; i++) if (output[i] != expected[i]) ExitProcess(8);
    for (unsigned i = 11; i < 16; i++) if (output[i] != 0x55) ExitProcess(9);
    if (*errno_cell != 123 || GetLastError() != 77) ExitProcess(10);
    if (remove("sample.bin") != -1 || *errno_cell != 13) ExitProcess(11);
    if (close(first) || remove("sample.bin") != -1 || *errno_cell != 13) ExitProcess(12);
    if (close(second) || remove("sample.bin")) ExitProcess(13);
    if (open("sample.bin", "rb") || *errno_cell != 2) ExitProcess(14);
    if (GetLastError() != 77) ExitProcess(15);
    ExitProcess(42);
}
