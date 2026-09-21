typedef __builtin_va_list va_list;
__declspec(dllimport) int __cdecl _vsnprintf(char *, unsigned int, const char *, va_list);
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static int render(char *output, unsigned int count, const char *format, ...) {
    va_list values;
    __builtin_va_start(values, format);
    int result = _vsnprintf(output, count, format, values);
    __builtin_va_end(values);
    return result;
}

static int same(const char *a, const char *b, unsigned int count) {
    for (unsigned int i = 0; i < count; ++i) if (a[i] != b[i]) return 0;
    return 1;
}

void entry(void) {
    char output[128];
    int *error = _errno();
    *error = 88;
    SetLastError(77);
    if (render(output, 128, "%s:%d:%X:%%", "ok", -42, 0xabcd) != 13) ExitProcess(1);
    if (!same(output, "ok:-42:ABCD:%", 14)) ExitProcess(2);
    if (render(output, 128, "%i/%u/%x", (-2147483647 - 1), 0xffffffffU, 0xabcd) != 27) ExitProcess(3);
    if (!same(output, "-2147483648/4294967295/abcd", 28)) ExitProcess(4);
    output[8] = '!';
    if (render(output, 8, "justfits") != 8 || output[8] != '!') ExitProcess(5);
    if (!same(output, "justfits", 8)) ExitProcess(6);
    if (render(output, 8, "muchlonger") != -1 || output[8] != '!') ExitProcess(7);
    if (!same(output, "muchlong", 8)) ExitProcess(8);
    if (render(output, 128, "%c%c", 0x12340041, 0) != 2) ExitProcess(9);
    if (output[0] != 'A' || output[1] || output[2]) ExitProcess(10);
    if (render(output, 0, "x") != -1 || output[0] != 'A') ExitProcess(11);
    if (render(output, 0, "") || render(0, 0, "%s-%u", "abc", 42) != 6) ExitProcess(12);
    if (*error != 88 || GetLastError() != 77) ExitProcess(13);
    ExitProcess(42);
}
