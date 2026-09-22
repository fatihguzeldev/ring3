typedef unsigned long Word;
typedef void *Key;
__declspec(dllimport) long __stdcall RegOpenKeyExA(Key, const char *, Word, Word, Key *);
__declspec(dllimport) long __stdcall RegCreateKeyExA(Key, const char *, Word, char *, Word, Word, void *, Key *, Word *);
__declspec(dllimport) long __stdcall RegCloseKey(Key);
__declspec(dllimport) long __stdcall RegSetValueExA(Key, const char *, Word, Word, const void *, Word);
__declspec(dllimport) long __stdcall RegQueryValueExA(Key, const char *, Word *, Word *, void *, Word *);
__declspec(dllimport) Word __stdcall GetLastError(void);
__declspec(dllimport) void __stdcall SetLastError(Word);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    Key root = (Key)0x80000001, key;
    Word value = 42, result = 77, type = 0, size = 0;
    char text[8];
    SetLastError(77);
    if (RegCreateKeyExA(root, "Software\\Example", 0, 0, 0, 3, 0, &key, 0)) ExitProcess(1);
    if (RegSetValueExA(key, "Setting", 0, 4, &value, 4)) ExitProcess(2);
    if (RegQueryValueExA(key, "SETTING", 0, &type, &result, &size) != 234 || size != 4 || type != 4 || result != 77) ExitProcess(3);
    if (RegQueryValueExA(key, "setting", 0, &type, &result, &size) || result != 42 || size != 4) ExitProcess(4);
    if (RegSetValueExA(key, 0, 0, 1, "Guest", 6)) ExitProcess(5);
    if (RegQueryValueExA(key, "", 0, &type, 0, &size) || size != 6 || type != 1) ExitProcess(6);
    if (RegCloseKey(key) || RegOpenKeyExA(root, "SOFTWARE\\EXAMPLE", 0, 1, &key)) ExitProcess(7);
    if (RegQueryValueExA(key, 0, 0, 0, text, &size) || text[0] != 'G' || text[4] != 't' || text[5] != 0) ExitProcess(8);
    if (RegSetValueExA(key, "Setting", 0, 4, &value, 4) != 5) ExitProcess(9);
    if (RegQueryValueExA(key, "Missing", 0, &type, &result, &size) != 2) ExitProcess(10);
    if (RegCloseKey(key) || GetLastError() != 77) ExitProcess(11);
    ExitProcess(42);
}
