#define _WIN32_WINNT 0x0601
#include <windows.h>
#include <stdio.h>

_Static_assert(sizeof(void *) == 4, "this oracle requires a native 32-bit build");

static void report(const char *operation, DWORD mode, BOOL result, DWORD error) {
    printf("{\"operation\":\"%s\",\"mode\":%lu,\"ok\":%u,\"error\":%lu}\n",
           operation, (unsigned long)mode, result != 0, (unsigned long)error);
}
static HMODULE fresh(const char *source, const char *path, DWORD mode) {
    DeleteFileA(path);
    if (!CopyFileA(source, path, FALSE)) return NULL;
    SetLastError(0);
    HMODULE module = LoadLibraryExA(path, NULL, mode);
    report("load", mode, module != NULL, GetLastError());
    return module;
}
static void probe(const char *source, DWORD mode) {
    const char *path = ".\\ring3-image-probe.dll";
    const char *renamed = ".\\ring3-image-renamed.dll";
    HMODULE module = fresh(source, path, mode);
    if (!module) return;
    const IMAGE_DOS_HEADER *dos = (const IMAGE_DOS_HEADER *)module;
    const IMAGE_NT_HEADERS32 *nt = (const IMAGE_NT_HEADERS32 *)
        ((const BYTE *)module + dos->e_lfanew);
    for (DWORD offset = 0; offset < nt->OptionalHeader.SizeOfImage; offset += 4096) {
        MEMORY_BASIC_INFORMATION info = {0};
        SIZE_T size = VirtualQuery((const BYTE *)module + offset, &info, sizeof(info));
        printf("{\"operation\":\"query\",\"mode\":%lu,\"offset\":%lu,"
               "\"size\":%lu,\"state\":%lu,\"protection\":%lu,\"type\":%lu,"
               "\"allocation_protection\":%lu,\"returned\":%lu}\n",
               (unsigned long)mode, (unsigned long)offset, (unsigned long)info.RegionSize,
               (unsigned long)info.State, (unsigned long)info.Protect,
               (unsigned long)info.Type, (unsigned long)info.AllocationProtect,
               (unsigned long)size);
    }
    SetLastError(0);
    HANDLE file = CreateFileA(path, GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
        OPEN_EXISTING, 0, NULL);
    report("open-write", mode, file != INVALID_HANDLE_VALUE, GetLastError());
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    SetLastError(0);
    BOOL result = DeleteFileA(path);
    report("delete-loaded", mode, result, GetLastError());
    DeleteFileA(renamed);
    SetLastError(0);
    result = MoveFileExA(path, renamed, 0);
    report("rename-loaded", mode, result, GetLastError());
    const char *current = result ? renamed : path;
    SetLastError(0);
    file = CreateFileA(current, DELETE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
        OPEN_EXISTING, 0, NULL);
    report("open-delete", mode, file != INVALID_HANDLE_VALUE, GetLastError());
    if (file != INVALID_HANDLE_VALUE) {
        FILE_DISPOSITION_INFO disposition = {TRUE};
        SetLastError(0);
        result = SetFileInformationByHandle(file, FileDispositionInfo,
                                           &disposition, sizeof(disposition));
        report("disposition-loaded", mode, result, GetLastError());
        CloseHandle(file);
    }
    SetLastError(0);
    file = CreateFileA(current, GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
        TRUNCATE_EXISTING, 0, NULL);
    report("truncate-loaded", mode, file != INVALID_HANDLE_VALUE, GetLastError());
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    SetLastError(0);
    result = FreeLibrary(module);
    report("free", mode, result, GetLastError());
    SetLastError(0);
    result = DeleteFileA(current);
    report("delete-unloaded", mode, result, GetLastError());
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    probe(argv[1], 0);
    probe(argv[1], DONT_RESOLVE_DLL_REFERENCES);
    return 0;
}
