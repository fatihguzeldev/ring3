__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned char output[32];
static const unsigned char expected[] = {
    0x78,0x56,0x34,0x12, 0x78,0x56,0x34,0x12, 0x78,0x56,0x34,0x12,
    0x78,0x56,0x78,0x56, 0x78,0x78,0x78,
    0x78,0x56,0x34,0x12, 0x78,0x56,0x78,
};

void entry(void) {
    for (int i = 0; i < 32; ++i) output[i] = 0xaa;
    unsigned char *cursor = output;
    unsigned int count = 3;
    __asm__ volatile("rep stosl" : "+D"(cursor), "+c"(count) : "a"(0x12345678) : "memory");
    if (count || cursor != output + 12) ExitProcess(1);
    count = 2;
    __asm__ volatile("rep stosw" : "+D"(cursor), "+c"(count) : "a"(0x12345678) : "memory");
    if (count || cursor != output + 16) ExitProcess(2);
    count = 3;
    __asm__ volatile("rep stosb" : "+D"(cursor), "+c"(count) : "a"(0x12345678) : "memory");
    if (count || cursor != output + 19) ExitProcess(3);
    __asm__ volatile("stosl; stosw; stosb" : "+D"(cursor) : "a"(0x12345678) : "memory");
    if (cursor != output + 26) ExitProcess(4);
    for (int i = 0; i < 26; ++i) if (output[i] != expected[i]) ExitProcess(5);
    for (int i = 26; i < 32; ++i) if (output[i] != 0xaa) ExitProcess(6);
    ExitProcess(42);
}
