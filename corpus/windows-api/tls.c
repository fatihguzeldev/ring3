__declspec(dllimport) unsigned long __stdcall TlsAlloc(void);
__declspec(dllimport) int __stdcall TlsFree(unsigned long);
__declspec(dllimport) void *__stdcall TlsGetValue(unsigned long);
__declspec(dllimport) int __stdcall TlsSetValue(unsigned long, void *);
__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    SetLastError(99);
    unsigned long index = TlsAlloc();
    if (index == 0xffffffff || GetLastError() != 99) ExitProcess(1);
    if (TlsGetValue(index) || GetLastError()) ExitProcess(2);
    SetLastError(99);
    if (!TlsSetValue(index, (void *)0x12345678) || GetLastError() != 99) ExitProcess(3);
    if (TlsGetValue(index) != (void *)0x12345678) ExitProcess(4);
    if (!TlsFree(index)) ExitProcess(5);
    index = TlsAlloc();
    if (index == 0xffffffff || TlsGetValue(index)) ExitProcess(6);
    if (!TlsFree(index)) ExitProcess(7);
    ExitProcess(42);
}
