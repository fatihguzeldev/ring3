__declspec(dllimport) void __cdecl _ftol(void);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const unsigned long long inputs[] = {
    0x3ffc000000000000ULL, 0xbffc000000000000ULL,
    0x41f00000001c0000ULL, 0xc1f00000001c0000ULL,
    0x43dfffffffffffffULL, 0xc3e0000000000000ULL,
};
static const unsigned int lows[] = {1, 0xffffffff, 1, 0xffffffff, 0xfffffc00, 0};
static const unsigned int highs[] = {0, 0xffffffff, 1, 0xfffffffe, 0x7fffffff, 0x80000000};

void entry(void) {
    unsigned short control = 0x027f;
    __asm__ volatile("fldcw %0" : : "m"(control));
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    for (int i = 0; i < 6; i++) {
        unsigned int low, high;
        __asm__ volatile("fldl %2; call *%3"
            : "=a"(low), "=d"(high) : "m"(inputs[i]), "r"(_ftol)
            : "st", "ecx", "cc", "memory");
        if (low != lows[i] || high != highs[i]) ExitProcess(1);
    }
    if (*error != 123 || GetLastError() != 77) ExitProcess(2);
    ExitProcess(42);
}
