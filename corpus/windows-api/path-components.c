typedef void (__cdecl *Split)(const char *, char *, char *, char *, char *);
typedef int *(__cdecl *Errno)(void);
__declspec(dllimport) void *__stdcall GetModuleHandleA(const char *);
__declspec(dllimport) void *__stdcall GetProcAddress(void *, const char *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static char drive[3], directory[256], filename[256], extension[256], long_name[256];
static const struct {
    const char *path, *drive, *directory, *filename, *extension;
} cases[] = {
    {"C:\\folder/sub.dir\\file.tar.gz", "C:", "\\folder/sub.dir\\", "file.tar", ".gz"},
    {"\\\\server\\share\\file", "", "\\\\server\\share\\", "file", ""},
    {".profile", "", "", "", ".profile"},
    {"a.b/", "", "a.b/", "", ""},
    {"", "", "", "", ""},
    {"1:f\377.\200", "1:", "", "f\377", ".\200"}
};

static int equal(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return *a == *b;
}

void entry(void) {
    void *crt = GetModuleHandleA("MSVCRT.dll");
    Split split = (Split)GetProcAddress(crt, "_splitpath");
    Errno error = (Errno)GetProcAddress(crt, "_errno");
    if (!split || !error) ExitProcess(1);
    int *errno_cell = error();
    *errno_cell = 123;
    SetLastError(77);
    for (unsigned i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        split(cases[i].path, drive, directory, filename, extension);
        if (!equal(drive, cases[i].drive) || !equal(directory, cases[i].directory)
            || !equal(filename, cases[i].filename) || !equal(extension, cases[i].extension)) ExitProcess(2);
    }
    for (unsigned mask = 0; mask < 16; mask++) {
        char *d = 0, *p = 0, *f = 0, *e = 0;
        if (mask & 1) d = drive;
        if (mask & 2) p = directory;
        if (mask & 4) f = filename;
        if (mask & 8) e = extension;
        drive[0] = directory[0] = filename[0] = extension[0] = '!';
        split("q:dir\\file.ext", d, p, f, e);
        if (mask & 1) { if (!equal(drive, "q:")) ExitProcess(3); }
        else if (drive[0] != '!') ExitProcess(3);
        if (mask & 2) { if (!equal(directory, "dir\\")) ExitProcess(4); }
        else if (directory[0] != '!') ExitProcess(4);
        if (mask & 4) { if (!equal(filename, "file")) ExitProcess(5); }
        else if (filename[0] != '!') ExitProcess(5);
        if (mask & 8) { if (!equal(extension, ".ext")) ExitProcess(6); }
        else if (extension[0] != '!') ExitProcess(6);
    }
    for (unsigned i = 0; i < 255; i++) long_name[i] = 'x';
    split(long_name, drive, directory, filename, extension);
    if (!equal(filename, long_name) || *drive || *directory || *extension) ExitProcess(7);
    if (*errno_cell != 123 || GetLastError() != 77) ExitProcess(8);
    ExitProcess(42);
}
