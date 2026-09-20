__declspec(dllimport) unsigned int __cdecl _controlfp(unsigned int, unsigned int);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned int word = 0;
    unsigned int replacement = 0x027d;
    if (_controlfp(0, 0) != 0x0009001f) ExitProcess(1);
    if (_controlfp(0x00020300, 0x00030300) != 0x000a031f) ExitProcess(2);
    __asm__ volatile ("fnstcw %0" : "+m" (word));
    if (word != 0x0c7f) ExitProcess(3);
    __asm__ volatile ("fldcw %0" : : "m" (replacement));
    if (_controlfp(0, 0) != 0x0001001f) ExitProcess(4);
    if (_controlfp(0x00080000, 0x00080000) != 0x0001001f) ExitProcess(5);
    ExitProcess(42);
}
