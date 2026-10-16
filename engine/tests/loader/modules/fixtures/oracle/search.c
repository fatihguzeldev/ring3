#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>

static void query(unsigned mode, unsigned phase, DWORD capacity) {
    WCHAR output[512];
    for (unsigned i = 0; i < 512; ++i) output[i] = 0xcccc;
    SetLastError(0);
    DWORD length = GetDllDirectoryW(capacity, capacity ? output : NULL);
    DWORD error = GetLastError();
    printf("{\"operation\":\"dll-directory-query\",\"mode\":%u,\"phase\":%u,\"capacity\":%lu,\"length\":%lu,\"error\":%lu,\"units\":[",
           mode, phase, (unsigned long)capacity, (unsigned long)length, (unsigned long)error);
    if (capacity) {
        unsigned count = length < capacity ? length + 1 : capacity;
        if (count > 512) count = 512;
        for (unsigned i = 0; i < count; ++i) printf("%s%u", i ? "," : "", (unsigned)output[i]);
    }
    puts("]}");
}
int main(void) {
    WCHAR original[32768], temporary[32768];
    if (!GetCurrentDirectoryW(32768, original) || !GetTempPathW(32768, temporary)) return 1;
    const WCHAR *paths[] = {NULL, L"", L"C:\\ring3-dll-directory-not-created", L"relative", L".", L"relative\\..\\child"};
    for (unsigned mode = 0; mode < sizeof(paths) / sizeof(paths[0]); ++mode) {
        if (!SetCurrentDirectoryW(original)) return 2;
        SetLastError(0);
        BOOL result = SetDllDirectoryW(paths[mode]);
        DWORD error = GetLastError();
        printf("{\"operation\":\"dll-directory-set\",\"mode\":%u,\"ok\":%d,\"error\":%lu}\n", mode, !!result, (unsigned long)error);
        query(mode, 0, 0);
        query(mode, 0, 3);
        query(mode, 0, 512);
        if (!SetCurrentDirectoryW(temporary)) return 3;
        query(mode, 1, 512);
    }
    SetDllDirectoryW(NULL);
    SetCurrentDirectoryW(original);
    return 0;
}
