typedef unsigned int u32;

_Static_assert(sizeof(u32) == 4, "u32 must occupy four bytes");

__declspec(dllimport) u32 __stdcall GetLastError(void);
__declspec(dllimport) void __stdcall SetLastError(u32 error);

volatile u32 ring3_result[3];

void ring3_entry(void) {
    SetLastError(0xf1234567u);
    ring3_result[0] = GetLastError();
    SetLastError(0);
    ring3_result[1] = GetLastError();
    ring3_result[2] = 0x52494e47u;
    for (;;) {
    }
}
