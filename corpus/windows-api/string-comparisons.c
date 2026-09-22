__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static const unsigned char left_bytes[] = {1, 2, 3, 4};
static const unsigned char right_bytes[] = {1, 2, 5, 4};
static const unsigned short left_words[] = {1, 3, 5};
static const unsigned short right_words[] = {2, 3, 7};
static const unsigned int left_dword = 0x80000000, right_dword = 1;

void entry(void) {
    const unsigned char *left = left_bytes, *right = right_bytes;
    unsigned int count = 4;
    unsigned char equal;
    __asm__ volatile("repe cmpsb; sete %3"
        : "+S"(left), "+D"(right), "+c"(count), "=qm"(equal) : : "cc", "memory");
    if (count != 1 || equal || left != left_bytes + 3 || right != right_bytes + 3) ExitProcess(1);
    const unsigned short *a = left_words, *b = right_words;
    count = 3;
    __asm__ volatile("repne cmpsw; sete %3"
        : "+S"(a), "+D"(b), "+c"(count), "=qm"(equal) : : "cc", "memory");
    if (count != 1 || !equal || a != left_words + 2 || b != right_words + 2) ExitProcess(2);
    const unsigned int *x = &left_dword, *y = &right_dword;
    unsigned char overflow, borrow;
    __asm__ volatile("cmpsl; seto %2; setb %3"
        : "+S"(x), "+D"(y), "=qm"(overflow), "=qm"(borrow) : : "cc", "memory");
    if (overflow != 1 || borrow || x != &left_dword + 1 || y != &right_dword + 1) ExitProcess(3);
    ExitProcess(42);
}
