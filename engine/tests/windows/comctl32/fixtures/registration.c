#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <commctrl.h>
#include <stdio.h>
#include <string.h>

typedef BOOL (WINAPI *init_ex)(const INITCOMMONCONTROLSEX *);

static void row(const char *phase, const wchar_t *name) {
    WNDCLASSW type;
    MEMORY_BASIC_INFORMATION page;
    BOOL loaded_before_query = GetModuleHandleW(L"comctl32.dll") != NULL;
    memset(&type, 0, sizeof(type));
    SetLastError(0);
    BOOL found = GetClassInfoW(NULL, name, &type);
    DWORD error = GetLastError();
    SIZE_T queried = found ? VirtualQuery((LPCVOID)type.lpfnWndProc, &page, sizeof(page)) : 0;
    printf("{\"operation\":\"comctl-registration\",\"phase\":\"%s\",\"loadedBeforeQuery\":%d,\"loaded\":%d,\"found\":%d,\"error\":%lu,\"procedureMapped\":%d,\"classStyle\":%u,\"windowExtra\":%d}\n",
           phase, loaded_before_query, GetModuleHandleW(L"comctl32.dll") != NULL, found, (unsigned long)error,
           queried != 0 && page.State == MEM_COMMIT, found ? type.style : 0,
           found ? type.cbWndExtra : 0);
}

int main(void) {
    row("before", PROGRESS_CLASSW);
    HMODULE module = LoadLibraryW(L"comctl32.dll");
    init_ex init = module ? (init_ex)(ULONG_PTR)GetProcAddress(module, "InitCommonControlsEx") : NULL;
    if (!module || !init) return 1;
    row("loaded", PROGRESS_CLASSW);
    const DWORD flags[] = { 0, ICC_PROGRESS_CLASS, ICC_PROGRESS_CLASS, 0x80000000u };
    for (unsigned i = 0; i < sizeof(flags) / sizeof(flags[0]); ++i) {
        INITCOMMONCONTROLSEX value = { sizeof(value), flags[i] };
        SetLastError(0x12345678);
        BOOL result = init(&value);
        DWORD error = GetLastError();
        printf("{\"operation\":\"comctl-init\",\"flags\":%lu,\"size\":%lu,\"result\":%d,\"error\":%lu}\n",
               (unsigned long)value.dwICC, (unsigned long)value.dwSize, result, (unsigned long)error);
    }
    for (unsigned size = 0; size <= 12; size += 4) {
        INITCOMMONCONTROLSEX value = { size, ICC_PROGRESS_CLASS };
        SetLastError(0x12345678);
        BOOL result = init(&value);
        DWORD error = GetLastError();
        printf("{\"operation\":\"comctl-init-size\",\"size\":%lu,\"result\":%d,\"error\":%lu}\n",
               (unsigned long)value.dwSize, result, (unsigned long)error);
    }
    row("initialized", PROGRESS_CLASSW);
    SetLastError(0);
    BOOL freed = FreeLibrary(module);
    DWORD error = GetLastError();
    printf("{\"operation\":\"comctl-free\",\"result\":%d,\"error\":%lu,\"loaded\":%d}\n",
           freed, (unsigned long)error, GetModuleHandleW(L"comctl32.dll") != NULL);
    row("freed", PROGRESS_CLASSW);
    return 0;
}
