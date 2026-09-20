__declspec(dllimport) unsigned int __cdecl value(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

typedef unsigned int (__cdecl *function)(void);
static unsigned int helper_calls;
static unsigned int __cdecl local_value(void) { return 42; }

void* __stdcall __delayLoadHelper2(const void* descriptor, function* slot) {
    (void)descriptor;
    helper_calls = helper_calls + 1;
    *slot = local_value;
    return (void*)local_value;
}

void entry(void) {
    unsigned int first = value();
    unsigned int second = value();
    if (helper_calls != 1) ExitProcess(1);
    ExitProcess(first + second - 42);
}
