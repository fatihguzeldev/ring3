__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    volatile unsigned int numerator = 0xfffffffe, divisor = 17;
    if (numerator / divisor != 252645134 || numerator % divisor != 16) ExitProcess(1);
    unsigned int low = 0x10003, high = 1;
    __asm__ volatile("divl %2" : "+a"(low), "+d"(high) : "m"(divisor) : "cc");
    if (low != 252648990 || high != 5) ExitProcess(2);
    unsigned short source = 257;
    low = 0xabcd0003;
    high = 0xface0001;
    __asm__ volatile("divw %2" : "+a"(low), "+d"(high) : "m"(source) : "cc");
    if (low != 0xabcd00ff || high != 0xface0004) ExitProcess(3);
    low = 0xabcd0190;
    __asm__ volatile("divb %%al" : "+a"(low) : : "cc");
    if (low != 0xabcd7002) ExitProcess(4);
    ExitProcess(42);
}
