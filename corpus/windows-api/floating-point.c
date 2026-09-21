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
    ExitProcess(42);
}
