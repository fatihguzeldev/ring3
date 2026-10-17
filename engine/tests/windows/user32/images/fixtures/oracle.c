#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static int rows;
static void row(const char *name) {
    if (rows++) putchar(',');
    printf("{\"case\":\"%s\"", name);
}
static HBITMAP pixels(int width, int height, DWORD **out) {
    BITMAPINFO info;
    memset(&info, 0, sizeof(info));
    info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    return CreateDIBSection(NULL, &info, DIB_RGB_COLORS, (void **)out, NULL, 0);
}
static void describe(const char *name, HICON image) {
    ICONINFO info;
    BITMAP mask, color;
    BOOL ok;
    memset(&info, 0, sizeof(info));
    memset(&mask, 0, sizeof(mask));
    memset(&color, 0, sizeof(color));
    SetLastError(0x12345678);
    ok = GetIconInfo(image, &info);
    row(name);
    printf(",\"ok\":%u,\"error\":%lu", ok != 0, GetLastError());
    if (ok) {
        GetObjectW(info.hbmMask, sizeof(mask), &mask);
        if (info.hbmColor) GetObjectW(info.hbmColor, sizeof(color), &color);
        printf(",\"icon\":%u,\"hotspot\":[%lu,%lu],\"mask\":[%ld,%ld,%u],\"color\":[%ld,%ld,%u]",
            info.fIcon != 0, info.xHotspot, info.yHotspot,
            mask.bmWidth, mask.bmHeight, mask.bmBitsPixel,
            color.bmWidth, color.bmHeight, color.bmBitsPixel);
        DeleteObject(info.hbmMask);
        if (info.hbmColor) DeleteObject(info.hbmColor);
    }
    putchar('}');
}
static HICON image(BOOL icon, BOOL alpha) {
    DWORD *bits;
    HBITMAP color = pixels(2, 2, &bits);
    const BYTE mask_bits[] = { 0xc0, 0, 0x40, 0 };
    HBITMAP mask = CreateBitmap(2, 2, 1, 1, mask_bits);
    ICONINFO info;
    HICON result;
    if (!color || !bits || !mask) ExitProcess(2);
    bits[0] = alpha ? 0x80800000 : 0x00ff0000;
    bits[1] = alpha ? 0xff00ff00 : 0x0000ff00;
    bits[2] = alpha ? 0x00000000 : 0x000000ff;
    bits[3] = alpha ? 0xffffffff : 0x00ffffff;
    memset(&info, 0, sizeof(info));
    info.fIcon = icon;
    info.xHotspot = 1;
    info.yHotspot = 0;
    info.hbmMask = mask;
    info.hbmColor = color;
    result = CreateIconIndirect(&info);
    if (!result) ExitProcess(2);
    DeleteObject(mask);
    DeleteObject(color);
    return result;
}
static void draw(const char *name, HICON source, UINT flags) {
    DWORD *bits;
    HBITMAP bitmap = pixels(2, 2, &bits);
    HDC dc = CreateCompatibleDC(NULL);
    HGDIOBJ old = SelectObject(dc, bitmap);
    BOOL ok;
    DWORD error;
    int i;
    if (!bitmap || !bits || !dc || !old) ExitProcess(2);
    for (i = 0; i < 4; ++i) bits[i] = 0x000000ff;
    SetLastError(0x12345678);
    ok = DrawIconEx(dc, 0, 0, source, 2, 2, 0, NULL, flags);
    error = GetLastError();
    GdiFlush();
    row(name);
    printf(",\"ok\":%u,\"error\":%lu,\"pixels\":[%lu,%lu,%lu,%lu]}",
        ok != 0, error, bits[0], bits[1], bits[2], bits[3]);
    SelectObject(dc, old);
    DeleteDC(dc);
    DeleteObject(bitmap);
}
#pragma pack(push, 2)
struct cursor_entry {
    WORD width, height, planes, depth;
    DWORD bytes;
    WORD id;
};
struct cursor_group {
    WORD reserved, type, count;
    struct cursor_entry entries[3];
};
#pragma pack(pop)
static void groups(void) {
    struct cursor_group group = { 0, 2, 3, {
        { 16, 32, 1, 1, 128, 101 },
        { 32, 32, 1, 32, 128, 102 },
        { 32, 64, 1, 32, 128, 103 }
    } };
    const int sizes[] = { 0, 16, 24, 32, 48, 64 };
    unsigned i, mono;
    for (mono = 0; mono < 2; ++mono) for (i = 0; i < sizeof(sizes) / sizeof(sizes[0]); ++i) {
        int result;
        SetLastError(0x12345678);
        result = LookupIconIdFromDirectoryEx((PBYTE)&group, FALSE, sizes[i], sizes[i], mono ? LR_MONOCHROME : 0);
        row("cursor-group-size");
        printf(",\"size\":%d,\"monochrome\":%u,\"id\":%d,\"error\":%lu}", sizes[i], mono, result, GetLastError());
    }
}
static void copies(void) {
    HICON original = image(TRUE, FALSE), copy;
    HCURSOR cursor = (HCURSOR)image(FALSE, FALSE);
    BOOL ok;
    describe("icon-original", original);
    describe("cursor-original", (HICON)cursor);
    copy = CopyImage(original, IMAGE_ICON, 4, 4, 0);
    describe("icon-stretched", copy);
    DestroyIcon(copy);
    copy = CopyImage(original, IMAGE_ICON, 2, 2, LR_COPYRETURNORG | LR_COPYDELETEORG);
    row("copy-return-original"); printf(",\"same\":%u}", copy == original);
    describe("copy-return-original-live", original);
    DestroyIcon(original);
    DestroyCursor(cursor);
    original = LoadIconW(NULL, IDI_APPLICATION);
    SetLastError(0x12345678);
    ok = DestroyIcon(original);
    row("shared-icon-destroy"); printf(",\"ok\":%u,\"error\":%lu}", ok != 0, GetLastError());
    describe("shared-icon-after-destroy", original);
}
int main(void) {
    HICON color = image(TRUE, FALSE), alpha = image(TRUE, TRUE);
    printf("{\"schema\":1,\"component\":\"user32-images\",\"pointerBytes\":%u,\"rows\":[", (unsigned)sizeof(void *));
    groups(); copies();
    draw("color-normal", color, DI_NORMAL);
    draw("color-mask", color, DI_MASK);
    draw("color-image", color, DI_IMAGE);
    draw("alpha-normal", alpha, DI_NORMAL);
    draw("alpha-mask", alpha, DI_MASK);
    draw("alpha-image", alpha, DI_IMAGE);
    DestroyIcon(color); DestroyIcon(alpha);
    puts("]}");
    return 0;
}
