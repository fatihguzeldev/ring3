__declspec(dllimport) void *__cdecl operator new(unsigned int);
__declspec(dllimport) void __cdecl operator delete(void *) noexcept;
extern "C" {
__declspec(dllimport) int *__cdecl _errno(void);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);
}

static unsigned int destroyed;
struct Value {
    unsigned int number;
    Value() : number(42) {}
    ~Value() { ++destroyed; }
};

extern "C" void entry(void) {
    int *error = _errno();
    *error = 123;
    SetLastError(77);
    Value *value = new Value;
    if (!value || value->number != 42) ExitProcess(1);
    delete value;
    if (destroyed != 1) ExitProcess(2);
    void *empty = ::operator new(0);
    if (!empty) ExitProcess(3);
    ::operator delete(empty);
    ::operator delete(nullptr);
    if (::operator new(0xffffffff)) ExitProcess(4);
    if (*error != 123 || GetLastError() != 77) ExitProcess(5);
    ExitProcess(42);
}
