__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    unsigned short control = 0x027f, status;
    unsigned long long one = 0x3ff0000000000000ULL;
    unsigned long long two = 0x4000000000000000ULL;
    unsigned long long negative_zero = 0x8000000000000000ULL;
    unsigned long long result;
    __asm__ volatile("fldcw %2; fldl %3; fldl %4; fldl %5; fst %%st(2); fstp %%st(1); fstp %%st(0); fstpl %0; fnstsw %1"
        : "=m"(result), "=m"(status)
        : "m"(control), "m"(one), "m"(two), "m"(negative_zero) : "st");
    if (result != negative_zero || status != 0) ExitProcess(1);
    unsigned long long ten = 0x4024000000000000ULL;
    __asm__ volatile("fldl %1; fdivrl %2; fstp %%st(0); fnstsw %0"
        : "=m"(status) : "m"(ten), "m"(one) : "st");
    if (status != 0x20) ExitProcess(2);
    ExitProcess(42);
}
