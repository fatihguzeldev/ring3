typedef unsigned long Word;
typedef void *Key;
typedef long (__stdcall *Open)(Key, const char *, Word, Word, Key *);
__declspec(dllimport) void *__stdcall LoadLibraryA(const char *);
__declspec(dllimport) void *__stdcall GetProcAddress(void *, const char *);
__declspec(dllimport) long __stdcall RegOpenKeyExA(Key, const char *, Word, Word, Key *);
__declspec(dllimport) long __stdcall RegCreateKeyExA(Key, const char *, Word, char *, Word, Word, void *, Key *, Word *);
__declspec(dllimport) long __stdcall RegCloseKey(Key);
__declspec(dllimport) Word __stdcall GetLastError(void);
__declspec(dllimport) void __stdcall SetLastError(Word);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    Key root = (Key)0x80000001, parent, child, reopened;
    Word disposition = 0;
    void *module = LoadLibraryA("ADVAPI32.dll");
    Open open = (Open)GetProcAddress(module, "RegOpenKeyExA");
    if (!module || open != RegOpenKeyExA) ExitProcess(1);
    SetLastError(77);
    if (open(root, "sOfTwArE", 0, 0x20019, &parent)) ExitProcess(2);
    if (RegCreateKeyExA(parent, "Example\\Settings", 0, 0, 0, 0x2001f, 0, &child, &disposition) || disposition != 1) ExitProcess(3);
    if (RegCloseKey(child) || RegCloseKey(child) != 6) ExitProcess(4);
    if (open(root, "SOFTWARE\\example\\settings", 0, 1, &reopened) || child == reopened) ExitProcess(5);
    if (RegCreateKeyExA(parent, "EXAMPLE\\SETTINGS", 0, 0, 0, 1, 0, &child, &disposition) || disposition != 2) ExitProcess(6);
    if (RegOpenKeyExA(parent, "Missing", 0, 1, &child) != 2) ExitProcess(7);
    if (RegCloseKey(child) || RegCloseKey(reopened) || RegCloseKey(parent) || RegCloseKey(root)) ExitProcess(8);
    if (GetLastError() != 77) ExitProcess(9);
    ExitProcess(42);
}
