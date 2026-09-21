__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned short control = 0x027f, saved;
    unsigned int low = 0x3f800000, high = 0x40000000;
    unsigned int ax;
    __asm__ volatile("fldcw %1; flds %2; fcoms %3; fnstsw %%ax"
        : "=a"(ax) : "m"(control), "m"(low), "m"(high) : "st");
    if ((ax & 0xffff) != 0x3900) ExitProcess(1);
    __asm__ volatile("fcomps %1; fnstsw %0" : "=m"(saved) : "m"(low) : "st");
    if (saved != 0x4000) ExitProcess(2);
    unsigned long long ten = 0x4024000000000000ULL;
    unsigned long long one = 0x3ff0000000000000ULL;
    unsigned long long zero = 0;
    __asm__ volatile("fldl %1; fdivrl %2; fnstsw %%ax"
        : "=a"(ax) : "m"(ten), "m"(one) : "st");
    if ((ax & 0x3a20) != 0x3a20) ExitProcess(3);
    __asm__ volatile("fcompl %1; fwait; fnstsw %0"
        : "=m"(saved) : "m"(zero) : "st");
    if (saved != 0x20) ExitProcess(4);
    unsigned long long negative = 0xbff8000000000000ULL;
    int integer;
    __asm__ volatile("fldl %2; fistpl %1; fnstsw %0"
        : "=m"(saved), "=m"(integer) : "m"(negative) : "st");
    if (saved != 0x220 || integer != -2) ExitProcess(5);
    __asm__ volatile("fildl %1; fnstsw %%ax" : "=a"(ax) : "m"(integer) : "st");
    if ((ax & 0xffff) != 0x3820) ExitProcess(6);
    unsigned long long output;
    __asm__ volatile("fstpl %1; fnstsw %0" : "=m"(saved), "=m"(output) : : "st");
    if (saved != 0x20 || output != 0xc000000000000000ULL) ExitProcess(7);
    ExitProcess(42);
}
