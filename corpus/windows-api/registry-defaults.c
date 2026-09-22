typedef void *HKEY;
__declspec(dllimport) long __stdcall RegSetValueA(HKEY, const char *, unsigned long, const char *, unsigned long);
__declspec(dllimport) long __stdcall RegOpenKeyExA(HKEY, const char *, unsigned long, unsigned long, HKEY *);
__declspec(dllimport) long __stdcall RegQueryValueExA(HKEY, const char *, unsigned long *, unsigned long *, unsigned char *, unsigned long *);
__declspec(dllimport) long __stdcall RegCloseKey(HKEY);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static void check(HKEY root, const char *path, const char *expected, unsigned long size) {
    HKEY key;
    unsigned long type = 0, capacity = 64, i;
    unsigned char bytes[64];
    if (RegOpenKeyExA(root, path, 0, 1, &key)) ExitProcess(1);
    if (RegQueryValueExA(key, 0, 0, &type, bytes, &capacity) || type != 1 || capacity != size) ExitProcess(2);
    for (i = 0; i < size; i++) if (bytes[i] != expected[i]) ExitProcess(3);
    if (RegCloseKey(key)) ExitProcess(4);
}

void entry(void) {
    HKEY user = (HKEY)0x80000001, machine = (HKEY)0x80000002, classes = (HKEY)0x80000000;
    SetLastError(77);
    if (RegSetValueA(user, "Software\\Sample\\Child", 1, "first", 0)) ExitProcess(5);
    check(user, "Software\\Sample\\Child", "first", sizeof "first");
    if (RegSetValueA(user, "software\\sample\\child", 1, "next", 0xffffffffUL)) ExitProcess(6);
    check(user, "Software\\Sample\\Child", "next", sizeof "next");
    if (RegSetValueA(classes, ".demo", 1, "Sample.Document", 1)) ExitProcess(7);
    check(machine, "Software\\Classes\\.demo", "Sample.Document", sizeof "Sample.Document");
    if (RegSetValueA(user, "Software\\Classes\\.demo", 1, "old", 0)) ExitProcess(8);
    if (RegSetValueA(classes, ".DEMO", 1, "new", 0)) ExitProcess(9);
    check(user, "Software\\Classes\\.demo", "new", sizeof "new");
    check(machine, "Software\\Classes\\.demo", "Sample.Document", sizeof "Sample.Document");
    if (GetLastError() != 77) ExitProcess(10);
    ExitProcess(42);
}
