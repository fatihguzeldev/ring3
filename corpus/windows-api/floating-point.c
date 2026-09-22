__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const int negative[] = {-2, -3, -2, -2};
static const int positive[] = {2, 1, 2, 1};

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
    unsigned short single = 0x007f;
    long long billion = 1000000000LL;
    unsigned int billion_float = 0x4e6e6b28;
    __asm__ volatile("fldcw %1; fildll %2; fmuls %3; fldcw %4; fstpl %0"
        : "=m"(output64)
        : "m"(single), "m"(billion), "m"(billion_float), "m"(control) : "st");
    if (output64 != 0x43abc16d60000000ULL) ExitProcess(10);
    unsigned short saved;
    __asm__ volatile("fwait; fnstcw %0; fwait" : "=m"(saved));
    if (saved != control) ExitProcess(6);
    unsigned long long fraction = 0xc004000000000000ULL;
    unsigned int positive_fraction = 0x3fe00000;
    for (unsigned int rc = 0; rc < 4; rc++) {
        unsigned short rounding = 0x027f | (rc << 10);
        short small;
        int medium;
        long long large;
        __asm__ volatile("fldcw %3; fldl %4; fists %0; fistl %1; fistpll %2"
            : "=m"(small), "=m"(medium), "=m"(large)
            : "m"(rounding), "m"(fraction) : "st");
        if (small != negative[rc] || medium != negative[rc] || large != negative[rc]) ExitProcess(7);
        int seven = 7;
        __asm__ volatile("fildl %2; fistps %0; flds %3; fistpl %1"
            : "=m"(small), "=m"(medium) : "m"(seven), "m"(positive_fraction) : "st");
        if (small != 7 || medium != positive[rc]) ExitProcess(8);
    }
    __asm__ volatile("fldcw %0" : : "m"(control));
    unsigned int eight = 0x41000000, two = 0x40000000;
    unsigned long long three = 0x4008000000000000ULL;
    __asm__ volatile("flds %1; fdivs %2; fdivl %3; fstpl %0"
        : "=m"(output64) : "m"(eight), "m"(two), "m"(three) : "st");
    if (output64 != 0x3ff5555555555555ULL) ExitProcess(9);
    ExitProcess(42);
}
