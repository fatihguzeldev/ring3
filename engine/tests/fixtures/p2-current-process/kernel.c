typedef unsigned int u32;

_Static_assert(sizeof(u32) == 4, "u32 must occupy four bytes");

__declspec(dllimport) u32 __stdcall GetCurrentProcess(void);
__declspec(dllimport) u32 __stdcall FlushInstructionCache(u32 process,
                                                      const void *base,
                                                      u32 size);
__declspec(dllimport) void __stdcall SetLastError(u32 error);
__declspec(dllimport) u32 __stdcall GetLastError(void);

u32 ring3_current_process(void) {
    u32 process = GetCurrentProcess();
    SetLastError(0x89abcdefu);
    u32 flushed = FlushInstructionCache(process, (const void *)0, 0xffffffffu);
    u32 error = GetLastError();
    u32 valid = (process == 0xffffffffu) & (flushed == 1u) &
                (error == 0x89abcdefu);
    return valid ? 42u : 0u;
}
