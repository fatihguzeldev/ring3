__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned short control = 0x027f;
    unsigned int input = 0x40800000, dividend = 0x41000000, output;
    __asm__ volatile("fldcw %1; flds %2; fsqrt; fdivrs %3; fstps %0"
        : "=m"(output) : "m"(control), "m"(input), "m"(dividend) : "st");
    if (output != 0x40800000) ExitProcess(1);
    unsigned long long input64 = 0x4010000000000000ULL;
    unsigned long long dividend64 = 0x4020000000000000ULL, output64;
    __asm__ volatile("fldl %1; fsqrt; fdivrl %2; fstpl %0"
        : "=m"(output64) : "m"(input64), "m"(dividend64) : "st");
    if (output64 != 0x4010000000000000ULL) ExitProcess(2);
    short integer16 = -7;
    unsigned int factor32 = 0x3fc00000;
    __asm__ volatile("filds %1; fmuls %2; fstpl %0"
        : "=m"(output64) : "m"(integer16), "m"(factor32) : "st");
    if (output64 != 0xc025000000000000ULL) ExitProcess(3);
    int integer32 = -2147483647 - 1;
    unsigned long long factor64 = 0x3fe0000000000000ULL;
    __asm__ volatile("fildl %1; fmull %2; fstpl %0"
        : "=m"(output64) : "m"(integer32), "m"(factor64) : "st");
    if (output64 != 0xc1d0000000000000ULL) ExitProcess(4);
    long long integer64 = 9007199254740992LL;
    __asm__ volatile("fildll %1; fmuls %2; fstpl %0"
        : "=m"(output64) : "m"(integer64), "m"(factor32) : "st");
    if (output64 != 0x4348000000000000ULL) ExitProcess(5);
    ExitProcess(42);
}
