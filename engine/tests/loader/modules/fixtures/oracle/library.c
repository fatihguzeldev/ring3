#include <windows.h>

__declspec(dllexport) DWORD WINAPI Ring3Value(void) { return 0x5233604; }
BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID reserved) {
    (void)module;
    (void)reason;
    (void)reserved;
    return TRUE;
}
