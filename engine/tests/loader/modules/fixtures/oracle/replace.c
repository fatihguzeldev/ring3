#define _WIN32_WINNT 0x0601
#include <windows.h>
#include <stddef.h>
#include <stdio.h>

_Static_assert(sizeof(void *) == 4, "this oracle requires a native 32-bit build");
_Static_assert(sizeof(FILE_BASIC_INFO) == 40, "pe32 basic layout");
_Static_assert(offsetof(FILE_RENAME_INFO, FileName) == 12, "pe32 rename layout");

static void record(const char *operation, DWORD mode, unsigned variant,
                   BOOL result, DWORD error) {
    printf("{\"operation\":\"%s\",\"mode\":%lu,\"variant\":%u,\"ok\":%u,\"error\":%lu}\n",
        operation, (unsigned long)mode, variant, result != 0, (unsigned long)error);
}
static void remove_files(void) {
    DeleteFileA(".\\ring3-replace-original.dll");
    DeleteFileA(".\\ring3-replace-new.dll");
    DeleteFileA(".\\ring3-replace-backup.dll");
    DeleteFileA(".\\ring3-replace-renamed.dll");
}
static void replace(const char *source, DWORD mode, unsigned variant) {
    const char *original = ".\\ring3-replace-original.dll";
    const char *replacement = ".\\ring3-replace-new.dll";
    const char *backup = ".\\ring3-replace-backup.dll";
    remove_files();
    if (!CopyFileA(source, original, FALSE) || !CopyFileA(source, replacement, FALSE)) return;
    HMODULE module = LoadLibraryExA(original, NULL, mode);
    if (!module) { record("load", mode, variant, FALSE, GetLastError()); return; }
    HMODULE backup_module = NULL;
    if (variant == 2) {
        if (!CopyFileA(source, backup, FALSE)) { FreeLibrary(module); return; }
        backup_module = LoadLibraryExA(backup, NULL, 0);
        if (!backup_module) { record("backup-load", mode, variant, FALSE, GetLastError()); FreeLibrary(module); return; }
    }
    SetLastError(0);
    BOOL result = ReplaceFileA(original, replacement, variant ? backup : NULL, 0, NULL, NULL);
    DWORD error = GetLastError();
    record("replace", mode, variant, result, error);
    printf("{\"operation\":\"paths\",\"mode\":%lu,\"variant\":%u,\"original\":%u,\"replacement\":%u,\"backup\":%u}\n",
        (unsigned long)mode, variant,
        GetFileAttributesA(original) != INVALID_FILE_ATTRIBUTES,
        GetFileAttributesA(replacement) != INVALID_FILE_ATTRIBUTES,
        GetFileAttributesA(backup) != INVALID_FILE_ATTRIBUTES);
    if (backup_module) FreeLibrary(backup_module);
    FreeLibrary(module);
    remove_files();
}
static void exclusion(const char *source, DWORD mode) {
    const char *path = ".\\ring3-replace-original.dll";
    remove_files();
    if (!CopyFileA(source, path, FALSE)) return;
    HMODULE module = LoadLibraryExA(path, NULL, mode);
    if (!module) { record("load", mode, 0, FALSE, GetLastError()); return; }
    SetLastError(0);
    HANDLE file = CreateFileA(path, GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL, OPEN_EXISTING, 0, NULL);
    record("open-write", mode, 0, file != INVALID_HANDLE_VALUE, GetLastError());
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    SetLastError(0);
    BOOL result = DeleteFileA(path);
    record("delete", mode, 0, result, GetLastError());
    if (!result) {
        SetLastError(0);
        result = MoveFileExA(path, ".\\ring3-replace-renamed.dll", 0);
        record("rename", mode, 0, result, GetLastError());
    }
    FreeLibrary(module);
    remove_files();
}
static void basic(void) {
    HANDLE file = CreateFileA(".\\ring3-basic.bin", GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL, CREATE_ALWAYS, 0, NULL);
    if (file == INVALID_HANDLE_VALUE) return;
    const LONGLONG values[] = {0, -1, -2, 123456};
    for (unsigned field = 0; field < 4; ++field) {
        for (unsigned row = 0; row < 4; ++row) {
            FILE_BASIC_INFO info = {0};
            LARGE_INTEGER *times[] = {&info.CreationTime, &info.LastAccessTime,
                                      &info.LastWriteTime, &info.ChangeTime};
            times[field]->QuadPart = values[row];
            SetLastError(0);
            BOOL result = SetFileInformationByHandle(file, FileBasicInfo, &info, sizeof(info));
            record("basic-time", field, row, result, GetLastError());
        }
    }
    FILE_ALLOCATION_INFO allocation = {0};
    allocation.AllocationSize.QuadPart = 131072;
    SetLastError(0);
    BOOL result = SetFileInformationByHandle(file, FileAllocationInfo, &allocation, sizeof(allocation));
    record("allocation", 0, 0, result, GetLastError());
    FILE_END_OF_FILE_INFO eof = {0};
    eof.EndOfFile.QuadPart = 1;
    SetLastError(0);
    result = SetFileInformationByHandle(file, FileEndOfFileInfo, &eof, sizeof(eof));
    record("eof", 0, 0, result, GetLastError());
    FILE_STANDARD_INFO standard = {0};
    if (GetFileInformationByHandleEx(file, FileStandardInfo, &standard, sizeof(standard)))
        printf("{\"operation\":\"allocation-after-eof\",\"allocation\":%lld,\"eof\":%lld}\n",
            standard.AllocationSize.QuadPart, standard.EndOfFile.QuadPart);
    CloseHandle(file);
    DeleteFileA(".\\ring3-basic.bin");
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    const DWORD modes[] = {0, DONT_RESOLVE_DLL_REFERENCES, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
                          LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE, LOAD_LIBRARY_AS_DATAFILE};
    for (unsigned index = 0; index < sizeof(modes) / sizeof(modes[0]); ++index) {
        exclusion(argv[1], modes[index]);
        for (unsigned variant = 0; variant < 3; ++variant) replace(argv[1], modes[index], variant);
    }
    basic();
    return 0;
}
