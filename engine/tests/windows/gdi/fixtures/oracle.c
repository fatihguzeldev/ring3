#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>

static void failed_dib(const char *name, BITMAPINFO *info, HANDLE section, DWORD offset, int comma) {
    void *bits = (void *)(ULONG_PTR)0x12345678;
    SetLastError(0xabcdef01);
    HBITMAP bitmap = CreateDIBSection(NULL, info, DIB_RGB_COLORS, &bits, section, offset);
    DWORD error = GetLastError();
    printf("%s{\"case\":\"%s\",\"created\":%u,\"bits\":%lu,\"error\":%lu}",
        comma ? "," : "", name, bitmap != NULL, (unsigned long)(ULONG_PTR)bits, error);
    if (bitmap) DeleteObject(bitmap);
}

int main(void) {
    const char *classes[] = {"Button", "Edit", "Static", "ListBox", "ScrollBar", "ComboBox"};
    HWND parent = CreateWindowExA(0, "Static", "parent", WS_POPUP, 0, 0, 100, 100, NULL, NULL, NULL, NULL);
    printf("{\"schema\":1,\"pointerBytes\":%u,\"classes\":[", (unsigned)sizeof(void *));
    for (unsigned i = 0; i < 6; ++i) {
        WNDCLASSA info = {0};
        HWND window = CreateWindowExA(0, classes[i], "text", WS_CHILD, 0, 0, 30, 20, parent, NULL, NULL, NULL);
        SetLastError(0);
        BOOL ordinal = GetClassInfoA(NULL, MAKEINTATOM(0x80 + i), &info);
        DWORD error = GetLastError();
        printf("%s{\"name\":\"%s\",\"created\":%u,\"atom\":%lu,\"ordinalLookup\":%u,\"ordinalError\":%lu}",
            i ? "," : "", classes[i], window != NULL, GetClassLongA(window, GCW_ATOM), ordinal, error);
        if (window) DestroyWindow(window);
    }
    BITMAPINFO info = {0};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = 2; info.bmiHeader.biHeight = -2;
    info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 16;
    void *bits = NULL;
    HBITMAP bitmap = CreateDIBSection(NULL, &info, DIB_RGB_COLORS, &bits, NULL, 0);
    HDC dc = CreateCompatibleDC(NULL);
    HGDIOBJ previous = SelectObject(dc, bitmap);
    const DWORD colors[] = {0x010101, 0x040404, 0x070707, 0x080808, 0x7f7f7f, 0xffffff, 0x202020, 0x404040, 0xc0c0c0};
    printf("],\"rgb555\":[");
    for (unsigned i = 0; i < sizeof(colors) / sizeof(colors[0]); ++i) {
        COLORREF actual = SetPixel(dc, 0, 0, colors[i]);
        printf("%s{\"color\":%lu,\"result\":%lu,\"raw\":%u}", i ? "," : "", colors[i], actual, bits ? *(WORD *)bits : 0);
    }
    SelectObject(dc, previous); DeleteObject(bitmap); DeleteDC(dc);
    HANDLE section = CreateFileMappingW(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 65536, NULL);
    BYTE *alias = MapViewOfFile(section, FILE_MAP_ALL_ACCESS, 0, 0, 65536);
    info.bmiHeader.biBitCount = 32;
    bitmap = CreateDIBSection(NULL, &info, DIB_RGB_COLORS, &bits, section, 4);
    DIBSECTION metadata = {0};
    int length = GetObjectW(bitmap, sizeof(metadata), &metadata);
    if (bits) *(DWORD *)bits = 0x12345678;
    printf("],\"section\":{\"created\":%u,\"objectBytes\":%d,\"sameHandle\":%u,\"offset\":%lu,\"alias\":%lu}",
        bitmap != NULL, length, metadata.dshSection == section, metadata.dsOffset, alias ? *(DWORD *)(alias + 4) : 0);
    DeleteObject(bitmap);
    printf(",\"failedDib\":[");
    failed_dib("invalidSection", &info, (HANDLE)(ULONG_PTR)0x1234, 0, 0);
    failed_dib("unalignedOffset", &info, section, 2, 1);
    HANDLE readonly = CreateFileMappingW(INVALID_HANDLE_VALUE, NULL, PAGE_READONLY, 0, 65536, NULL);
    failed_dib("readonlySection", &info, readonly, 0, 1);
    if (readonly) CloseHandle(readonly);
    printf("]");
    if (alias) { *(DWORD *)(alias + 4) = 7; UnmapViewOfFile(alias); }
    CloseHandle(section);
    DestroyWindow(parent);
    puts("}");
    return 0;
}
