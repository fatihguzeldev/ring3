#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static void failed_dib(const char *name, BITMAPINFO *info, HANDLE section, DWORD offset, int comma) {
    void *bits = (void *)(ULONG_PTR)0x12345678;
    SetLastError(0xabcdef01);
    HBITMAP bitmap = CreateDIBSection(NULL, info, DIB_RGB_COLORS, &bits, section, offset);
    DWORD error = GetLastError();
    printf("%s{\"case\":\"%s\",\"created\":%u,\"bits\":%lu,\"error\":%lu}",
        comma ? "," : "", name, bitmap != NULL, (unsigned long)(ULONG_PTR)bits, error);
    if (bitmap) DeleteObject(bitmap);
}


static unsigned long quad_color(RGBQUAD value) {
    return RGB(value.rgbRed, value.rgbGreen, value.rgbBlue);
}

static void system_brush_reference(void) {
    HBRUSH brush = GetSysColorBrush(COLOR_BTNFACE);
    LOGBRUSH before = {0}, after = {0};
    int before_bytes = GetObjectW(brush, sizeof(before), &before);
    SetLastError(0xabcdef01);
    BOOL deleted = DeleteObject(brush);
    DWORD error = GetLastError();
    HBRUSH repeated = GetSysColorBrush(COLOR_BTNFACE);
    int after_bytes = GetObjectW(brush, sizeof(after), &after);
    printf(",\"systemBrush\":{\"cached\":%u,\"deleteResult\":%u,\"deleteError\":%lu,\"beforeBytes\":%d,\"afterBytes\":%d,\"beforeColor\":%lu,\"afterColor\":%lu,\"queriedColor\":%lu}",
        brush == repeated, deleted, error, before_bytes, after_bytes, before.lbColor, after.lbColor, GetSysColor(COLOR_BTNFACE));
}

static void bitmap_masks_reference(void) {
    struct HeaderMasks { BITMAPINFOHEADER header; DWORD masks[3]; } info = {0};
    const unsigned bits[] = {16, 24, 32, 16, 16, 32};
    const DWORD compression[] = {BI_RGB, BI_RGB, BI_RGB, BI_BITFIELDS, BI_BITFIELDS, BI_BITFIELDS};
    const DWORD masks[][3] = {{0,0,0}, {0,0,0}, {0,0,0}, {0x7c00,0x3e0,0x1f}, {0xf800,0x7e0,0x1f}, {0xff,0xff00,0xff0000}};
    printf(",\"bitmapMasks\":[");
    for (unsigned i = 0; i < sizeof(bits)/sizeof(bits[0]); ++i) {
        info.header.biSize = sizeof(BITMAPINFOHEADER);
        info.header.biWidth = 2; info.header.biHeight = -1;
        info.header.biPlanes = 1; info.header.biBitCount = (WORD)bits[i];
        info.header.biCompression = compression[i];
        memcpy(info.masks, masks[i], sizeof(info.masks));
        void *pixels = NULL;
        HBITMAP bitmap = CreateDIBSection(NULL, (BITMAPINFO *)&info, DIB_RGB_COLORS, &pixels, NULL, 0);
        DIBSECTION object = {0};
        int size = GetObjectW(bitmap, sizeof(object), &object);
        HDC dc = CreateCompatibleDC(NULL);
        HGDIOBJ previous = SelectObject(dc, bitmap);
        COLORREF actual = SetPixel(dc, 0, 0, 0x332211);
        DWORD raw = 0;
        if (pixels) memcpy(&raw, pixels, bits[i]/8);
        printf("%s{\"bits\":%u,\"compression\":%lu,\"created\":%u,\"objectBytes\":%d,\"objectCompression\":%lu,\"masks\":[%lu,%lu,%lu],\"pixelResult\":%lu,\"raw\":%lu}",
            i ? "," : "", bits[i], compression[i], bitmap != NULL, size, object.dsBmih.biCompression,
            object.dsBitfields[0], object.dsBitfields[1], object.dsBitfields[2], actual, raw);
        SelectObject(dc, previous); DeleteDC(dc); DeleteObject(bitmap);
    }
    printf("]");
}

static void logical_palette_reference(void) {
    struct Palette3 { WORD version, count; PALETTEENTRY entries[3]; } logical = {0x300, 3, {{255,0,0,0},{0,255,0,0},{0,0,255,0}}};
    struct IndexedInfo { BITMAPINFOHEADER header; WORD indexes[4]; } info = {0};
    info.header.biSize = sizeof(BITMAPINFOHEADER);
    info.header.biWidth = 2; info.header.biHeight = -1;
    info.header.biPlanes = 1; info.header.biBitCount = 8; info.header.biClrUsed = 3;
    info.indexes[0] = 2; info.indexes[1] = 0; info.indexes[2] = 1;
    HPALETTE palette = CreatePalette((LOGPALETTE *)&logical);
    HDC dc = CreateCompatibleDC(NULL);
    HPALETTE old_palette = SelectPalette(dc, palette, FALSE);
    void *pixels = NULL;
    HBITMAP bitmap = CreateDIBSection(dc, (BITMAPINFO *)&info, DIB_PAL_COLORS, &pixels, NULL, 0);
    HGDIOBJ previous = SelectObject(dc, bitmap);
    RGBQUAD before[3] = {0}, changed[3] = {0}, realized[3] = {0};
    UINT before_count = GetDIBColorTable(dc, 0, 3, before);
    PALETTEENTRY replacement = {0x11,0x22,0x33,0};
    UINT set_count = SetPaletteEntries(palette, 0, 1, &replacement);
    UINT changed_count = GetDIBColorTable(dc, 0, 3, changed);
    UINT realization = RealizePalette(dc);
    UINT realized_count = GetDIBColorTable(dc, 0, 3, realized);
    printf(",\"logicalPalette\":{\"created\":%u,\"selected\":%u,\"dibCreated\":%u,\"counts\":[%u,%u,%u,%u],\"realization\":%u,\"before\":[%lu,%lu,%lu],\"changed\":[%lu,%lu,%lu],\"realized\":[%lu,%lu,%lu]}",
        palette != NULL, old_palette != NULL, bitmap != NULL, before_count, set_count, changed_count, realized_count, realization,
        quad_color(before[0]), quad_color(before[1]), quad_color(before[2]),
        quad_color(changed[0]), quad_color(changed[1]), quad_color(changed[2]),
        quad_color(realized[0]), quad_color(realized[1]), quad_color(realized[2]));
    SelectObject(dc, previous); DeleteObject(bitmap);
    SelectPalette(dc, old_palette, FALSE); DeleteDC(dc); DeleteObject(palette);
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
    system_brush_reference();
    bitmap_masks_reference();
    logical_palette_reference();
    DestroyWindow(parent);
    puts("}");
    return 0;
}
