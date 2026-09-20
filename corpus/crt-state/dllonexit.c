typedef int (__cdecl *Callback)(void);
__declspec(dllimport) void *__cdecl malloc(unsigned int);
__declspec(dllimport) void __cdecl free(void *);
__declspec(dllimport) Callback __cdecl __dllonexit(Callback, Callback **, Callback **);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned int trace;
static int first(void) { trace = trace * 10 + 1; return 0; }
static int second(void) { trace = trace * 10 + 2; return 0; }

void entry(void) {
    Callback *begin = malloc(sizeof(Callback));
    if (!begin) ExitProcess(1);
    Callback *end = begin;
    if (__dllonexit(first, &begin, &end) != first) ExitProcess(2);
    if (__dllonexit(second, &begin, &end) != second) ExitProcess(3);
    if (trace || end - begin != 2 || begin[0] != first || begin[1] != second) ExitProcess(4);
    while (end != begin) { --end; (*end)(); }
    if (trace != 21) ExitProcess(5);
    free(begin);
    ExitProcess(42);
}
