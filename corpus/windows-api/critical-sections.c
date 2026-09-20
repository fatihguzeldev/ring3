typedef struct { unsigned long reserved[6]; } CriticalSection;
_Static_assert(sizeof(CriticalSection) == 24, "x86 critical section size");
__declspec(dllimport) void __stdcall InitializeCriticalSection(CriticalSection *);
__declspec(dllimport) void __stdcall EnterCriticalSection(CriticalSection *);
__declspec(dllimport) int __stdcall TryEnterCriticalSection(CriticalSection *);
__declspec(dllimport) void __stdcall LeaveCriticalSection(CriticalSection *);
__declspec(dllimport) void __stdcall DeleteCriticalSection(CriticalSection *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static CriticalSection section;

void entry(void) {
    SetLastError(99);
    InitializeCriticalSection(&section);
    EnterCriticalSection(&section);
    if (!TryEnterCriticalSection(&section)) ExitProcess(1);
    LeaveCriticalSection(&section);
    LeaveCriticalSection(&section);
    DeleteCriticalSection(&section);
    InitializeCriticalSection(&section);
    DeleteCriticalSection(&section);
    if (GetLastError() != 99) ExitProcess(2);
    ExitProcess(42);
}
