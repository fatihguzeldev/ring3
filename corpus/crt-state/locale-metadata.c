__declspec(dllimport) extern unsigned long __lc_handle[6];
__declspec(dllimport) extern unsigned int __lc_codepage;
__declspec(dllimport) extern int __lc_collate_cp;
__declspec(dllimport) extern int __mb_cur_max;
__declspec(dllimport) int __cdecl _setmbcp(int);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned short widen_c_byte(unsigned char value) {
    if (__lc_handle[2] || __lc_codepage || __mb_cur_max != 1) ExitProcess(1);
    return value;
}

void entry(void) {
    for (int i = 0; i < 6; ++i) {
        if (__lc_handle[i]) ExitProcess(2);
    }
    if (__lc_codepage || __lc_collate_cp || __mb_cur_max != 1) ExitProcess(3);
    if (_setmbcp(437) != 0) ExitProcess(4);
    if (__lc_handle[2] || __lc_codepage || __lc_collate_cp || __mb_cur_max != 1)
        ExitProcess(5);
    if (widen_c_byte(0) != 0 || widen_c_byte(0x41) != 0x41 ||
        widen_c_byte(0x80) != 0x80 || widen_c_byte(0xff) != 0xff) ExitProcess(6);
    ExitProcess(42);
}
