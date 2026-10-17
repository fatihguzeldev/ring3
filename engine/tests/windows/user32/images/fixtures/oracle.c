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
static void stock_metadata(void) {
    const WORD bitmap_ids[] = { 32734,32735,32736,32737,32738,32739,32740,32741,
        32742,32743,32744,32745,32746,32747,32748,32749,32750,32751,32752,32753,
        32754,32755,32756,32757,32758,32759,32760,32761,32762,32763,32764,32765,32766,32767 };
    const WORD cursor_ids[] = { 32512,32513,32514,32515,32516,32640,32641,32642,
        32643,32644,32645,32646,32648,32649,32650,32651 };
    unsigned i;
    for (i=0;i<sizeof(bitmap_ids)/sizeof(bitmap_ids[0]);++i) {
        HBITMAP first,second;
        BITMAP info;
        DWORD error;
        int count;
        SetLastError(0x12345678);
        first=LoadBitmapW(NULL,MAKEINTRESOURCEW(bitmap_ids[i]));
        error=GetLastError();
        second=LoadBitmapW(NULL,MAKEINTRESOURCEW(bitmap_ids[i]));
        memset(&info,0,sizeof(info));
        count=GetObjectW(first,sizeof(info),&info);
        row("stock-bitmap-metadata");
        printf(",\"id\":%u,\"present\":%u,\"error\":%lu,\"sameHandle\":%u,\"objectBytes\":%d,\"width\":%ld,\"height\":%ld,\"rowBytes\":%ld,\"planes\":%u,\"bits\":%u}",
            bitmap_ids[i],first!=NULL,error,first==second,count,info.bmWidth,info.bmHeight,info.bmWidthBytes,info.bmPlanes,info.bmBitsPixel);
        if(second && second!=first)DeleteObject(second);
        if(first)DeleteObject(first);
    }
    for(i=0;i<sizeof(cursor_ids)/sizeof(cursor_ids[0]);++i) {
        char label[64];
        sprintf_s(label,sizeof(label),"stock-cursor-%u",cursor_ids[i]);
        describe(label,(HICON)LoadCursorW(NULL,MAKEINTRESOURCEW(cursor_ids[i])));
    }
}

#pragma pack(push, 2)
struct icon_entry {
    BYTE width, height, colors, reserved;
    WORD planes, depth;
    DWORD bytes;
    WORD id;
};
struct icon_group {
    WORD reserved, type, count;
    struct icon_entry entries[3];
};
#pragma pack(pop)
static void selector_matrix(void) {
    struct cursor_group groups[] = {
        { 0, 2, 3, { {16,32,1,32,128,201}, {48,96,1,32,128,202}, {32,64,1,32,128,203} } },
        { 0, 2, 3, { {48,96,1,32,128,202}, {16,32,1,32,128,201}, {32,64,1,32,128,203} } },
        { 0, 2, 3, { {16,16,1,32,128,301}, {48,48,1,32,128,302}, {32,32,1,32,128,303} } },
        { 0, 2, 3, { {32,64,1,1,128,401}, {32,64,1,8,128,402}, {32,64,1,32,128,403} } }
    };
    struct icon_group icons = { 0, 1, 3, {
        {16,16,0,0,1,32,128,501}, {48,48,0,0,1,32,128,502}, {32,32,0,0,1,32,128,503}
    } };
    const int sizes[] = { 0, 16, 24, 32, 48, 64 };
    unsigned group, size, mono;
    for (group = 0; group < sizeof(groups) / sizeof(groups[0]); ++group)
    for (mono = 0; mono < 2; ++mono)
    for (size = 0; size < sizeof(sizes) / sizeof(sizes[0]); ++size) {
        int result;
        SetLastError(0x12345678);
        result = LookupIconIdFromDirectoryEx((PBYTE)&groups[group], FALSE,
            sizes[size], sizes[size], mono ? LR_MONOCHROME : 0);
        row("cursor-selector-matrix");
        printf(",\"group\":%u,\"size\":%d,\"monochrome\":%u,\"id\":%d,\"error\":%lu}",
            group, sizes[size], mono, result, GetLastError());
    }
    for (mono = 0; mono < 2; ++mono)
    for (size = 0; size < sizeof(sizes) / sizeof(sizes[0]); ++size) {
        int result;
        SetLastError(0x12345678);
        result = LookupIconIdFromDirectoryEx((PBYTE)&icons, TRUE,
            sizes[size], sizes[size], mono ? LR_MONOCHROME : 0);
        row("icon-selector-control");
        printf(",\"size\":%d,\"monochrome\":%u,\"id\":%d,\"error\":%lu}",
            sizes[size], mono, result, GetLastError());
    }
}
static void palette_flags(void) {
    WCHAR directory[MAX_PATH], path[MAX_PATH];
    BITMAPFILEHEADER header;
    struct {
        BITMAPINFOHEADER header;
        RGBQUAD colors[8];
    } info;
    const DWORD colors[] = {0x00ffffff,0,0x00ff0000,0x0000ff00,
        0x000000ff,0x00808080,0x0017395b,0x00a2c4e6};
    const BYTE indices[] = {4,5,6,7,0,1,2,3};
    const UINT flags[] = {0,LR_MONOCHROME,LR_VGACOLOR,LR_CREATEDIBSECTION,
        LR_MONOCHROME|LR_CREATEDIBSECTION,LR_VGACOLOR|LR_CREATEDIBSECTION};
    HANDLE file;
    DWORD written;
    unsigned i, pixel;
    HDC dc = CreateCompatibleDC(NULL);
    if (!dc || !GetTempPathW(MAX_PATH,directory) || !GetTempFileNameW(directory,L"r3i",0,path)) ExitProcess(2);
    memset(&header,0,sizeof(header));
    memset(&info,0,sizeof(info));
    header.bfType = 0x4d42;
    header.bfOffBits = sizeof(header) + sizeof(info);
    header.bfSize = header.bfOffBits + sizeof(indices);
    info.header.biSize = sizeof(info.header);
    info.header.biWidth = 4; info.header.biHeight = 2;
    info.header.biPlanes = 1; info.header.biBitCount = 8;
    info.header.biClrUsed = 8; info.header.biSizeImage = sizeof(indices);
    for (i=0;i<8;++i) {
        info.colors[i].rgbRed = (BYTE)(colors[i]>>16);
        info.colors[i].rgbGreen = (BYTE)(colors[i]>>8);
        info.colors[i].rgbBlue = (BYTE)colors[i];
    }
    file = CreateFileW(path,GENERIC_WRITE,0,NULL,CREATE_ALWAYS,FILE_ATTRIBUTE_TEMPORARY,NULL);
    if (file == INVALID_HANDLE_VALUE ||
        !WriteFile(file,&header,sizeof(header),&written,NULL) || written != sizeof(header) ||
        !WriteFile(file,&info,sizeof(info),&written,NULL) || written != sizeof(info) ||
        !WriteFile(file,indices,sizeof(indices),&written,NULL) || written != sizeof(indices)) ExitProcess(2);
    CloseHandle(file);
    for (i=0;i<sizeof(flags)/sizeof(flags[0]);++i) {
        BITMAP bitmap;
        BITMAPINFO output;
        DWORD pixels[8] = {0};
        DWORD error;
        int lines = 0;
        HBITMAP loaded;
        memset(&bitmap,0,sizeof(bitmap));
        memset(&output,0,sizeof(output));
        output.bmiHeader.biSize = sizeof(output.bmiHeader);
        output.bmiHeader.biWidth = 4; output.bmiHeader.biHeight = -2;
        output.bmiHeader.biPlanes = 1; output.bmiHeader.biBitCount = 32;
        SetLastError(0x12345678);
        loaded = LoadImageW(NULL,path,IMAGE_BITMAP,0,0,LR_LOADFROMFILE|flags[i]);
        error = GetLastError();
        if (loaded) {
            if (!GetObjectW(loaded,sizeof(bitmap),&bitmap)) ExitProcess(2);
            lines = GetDIBits(dc,loaded,0,2,pixels,&output,DIB_RGB_COLORS);
            if (lines != 2) ExitProcess(2);
        }
        row("indexed-bitmap-flags");
        printf(",\"flags\":%u,\"ok\":%u,\"error\":%lu,\"depth\":%u,\"dib\":%u,\"lines\":%d,\"pixels\":[",
            flags[i],loaded!=NULL,error,bitmap.bmBitsPixel,bitmap.bmBits!=NULL,lines);
        for (pixel=0;pixel<8;++pixel) printf("%s%lu",pixel?",":"",pixels[pixel]);
        printf("]}");
        if (loaded) DeleteObject(loaded);
    }
    DeleteFileW(path);
    DeleteDC(dc);
    {
        HICON original = image(TRUE,FALSE);
        HICON copy = CopyImage(original,IMAGE_ICON,0,0,LR_MONOCHROME);
        if (!copy) ExitProcess(2);
        describe("color-icon-monochrome-copy",copy);
        draw("color-icon-monochrome-normal",copy,DI_NORMAL);
        draw("color-icon-monochrome-image",copy,DI_IMAGE);
        DestroyIcon(copy); DestroyIcon(original);
    }
}

int main(void) {
    HICON color = image(TRUE, FALSE), alpha = image(TRUE, TRUE);
    printf("{\"schema\":1,\"component\":\"user32-images\",\"pointerBytes\":%u,\"rows\":[", (unsigned)sizeof(void *));
    groups(); copies();
    selector_matrix(); palette_flags(); stock_metadata();
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
