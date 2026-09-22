__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    volatile int left = -2147483647 - 1, right = 2;
    long long product = (long long)left * right;
    if (product != -4294967296LL) ExitProcess(1);
    unsigned int low = 0xabcd8000, high = 0x12345678;
    unsigned short source = 2;
    unsigned char overflow;
    __asm__ volatile("imulw %3; seto %2"
        : "+a"(low), "+d"(high), "=qm"(overflow) : "m"(source) : "cc");
    if (low != 0xabcd0000 || high != 0x1234ffff || overflow != 1) ExitProcess(2);
    low = 0xabcd03fe;
    __asm__ volatile("imulb %%ah; seto %1" : "+a"(low), "=qm"(overflow) : : "cc");
    if (low != 0xabcdfffa || overflow != 0) ExitProcess(3);
    low = 0x80000000;
    high = 0xffffffff;
    __asm__ volatile("imull %%edx; seto %2"
        : "+a"(low), "+d"(high), "=qm"(overflow) : : "cc");
    if (low != 0x80000000 || high != 0 || overflow != 1) ExitProcess(4);
    ExitProcess(42);
}
