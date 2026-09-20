__declspec(dllimport) extern unsigned int state;
__declspec(dllimport) unsigned int __cdecl value(void);
__declspec(dllimport) unsigned int __cdecl ordinal_value(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    if (state != 40) ExitProcess(1);
    if (value() != 41) ExitProcess(2);
    ExitProcess(ordinal_value());
}
