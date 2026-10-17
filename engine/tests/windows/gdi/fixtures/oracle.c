#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#pragma comment(lib, "msimg32.lib")

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

static void synthesized_palette_reference(void) {
    DWORD input[2] = {0, 0xffffff};
    HBITMAP bitmap = CreateBitmap(2, 1, 1, 32, input);
    HDC dc = CreateCompatibleDC(NULL);
    const WORD depths[] = {1, 4, 8};
    printf(",\"synthesizedPalette\":[");
    for (unsigned usage = 0; usage <= 1; ++usage) {
        for (unsigned i = 0; i < 3; ++i) {
            struct OutputInfo { BITMAPINFOHEADER header; BYTE table[1024]; } info;
            memset(&info, 0x5a, sizeof(info));
            memset(&info.header, 0, sizeof(info.header));
            info.header.biSize = sizeof(info.header);
            info.header.biWidth = 2; info.header.biHeight = -1;
            info.header.biPlanes = 1; info.header.biBitCount = depths[i];
            BYTE pixels[4] = {0x5a,0x5a,0x5a,0x5a};
            SetLastError(0xabcdef01);
            int count = GetDIBits(dc, bitmap, 0, 1, pixels, (BITMAPINFO *)&info, usage);
            DWORD error = GetLastError();
            printf("%s{\"bits\":%u,\"usage\":%u,\"lines\":%d,\"error\":%lu,\"used\":%lu,\"imageBytes\":%lu,\"pixels\":[%u,%u,%u,%u],\"table\":[",
                usage || i ? "," : "", depths[i], usage, count, error, info.header.biClrUsed,
                info.header.biSizeImage, pixels[0], pixels[1], pixels[2], pixels[3]);
            unsigned entries = 1u << depths[i];
            for (unsigned j = 0; j < entries; ++j) {
                unsigned long value;
                if (usage == DIB_PAL_COLORS) { WORD word; memcpy(&word, info.table + j * 2, 2); value = word; }
                else { DWORD word; memcpy(&word, info.table + j * 4, 4); value = word; }
                printf("%s%lu", j ? "," : "", value);
            }
            printf("]}");
        }
    }
    printf("]");
    PALETTEENTRY defaults[20] = {{0}};
    UINT default_count = GetPaletteEntries((HPALETTE)GetStockObject(DEFAULT_PALETTE), 0, 20, defaults);
    printf(",\"defaultPalette\":{\"count\":%u,\"entries\":[", default_count);
    for (unsigned i = 0; i < default_count; ++i) {
        DWORD value; memcpy(&value, &defaults[i], 4);
        printf("%s%lu", i ? "," : "", value);
    }
    printf("]},\"paletteLimits\":[");
    const unsigned sizes[] = {0,1,256,1024,1025};
    for (unsigned i = 0; i < sizeof(sizes)/sizeof(sizes[0]); ++i) {
        LOGPALETTE *logical = (LOGPALETTE *)calloc(1, 4 + sizes[i] * sizeof(PALETTEENTRY));
        logical->palVersion = 0x300; logical->palNumEntries = (WORD)sizes[i];
        SetLastError(0xabcdef01);
        HPALETTE palette = CreatePalette(logical);
        DWORD error = GetLastError();
        printf("%s{\"entries\":%u,\"created\":%u,\"error\":%lu}", i ? "," : "", sizes[i], palette != NULL, error);
        if (palette) DeleteObject(palette);
        free(logical);
    }
    printf("]");
    DeleteDC(dc); DeleteObject(bitmap);
}

static void indexed_ddb_reference(void) {
    const WORD depths[] = {4, 8};
    printf(",\"indexedDdb\":[");
    for (unsigned i = 0; i < 2; ++i) {
        BYTE input[4] = {0,0,0,0};
        if (depths[i] == 4) input[0] = 0x12;
        else { input[0] = 1; input[1] = 2; }
        HBITMAP bitmap = CreateBitmap(2, 1, 1, depths[i], input);
        HDC dc = CreateCompatibleDC(NULL);
        HGDIOBJ previous = SelectObject(dc, bitmap);
        COLORREF first = GetPixel(dc, 0, 0), second = GetPixel(dc, 1, 0);
        SelectObject(dc, previous);
        struct IndexedDdbInfo { BITMAPINFOHEADER header; RGBQUAD table[256]; } info = {0};
        info.header.biSize = sizeof(info.header);
        info.header.biWidth = 2; info.header.biHeight = -1;
        info.header.biPlanes = 1; info.header.biBitCount = depths[i];
        BYTE pixels[4] = {0x5a,0x5a,0x5a,0x5a};
        SetLastError(0xabcdef01);
        int count = GetDIBits(dc, bitmap, 0, 1, pixels, (BITMAPINFO *)&info, DIB_RGB_COLORS);
        DWORD error = GetLastError();
        printf("%s{\"bits\":%u,\"created\":%u,\"pixelColors\":[%lu,%lu],\"lines\":%d,\"error\":%lu,\"used\":%lu,\"pixels\":[%u,%u,%u,%u],\"table\":[",
            i ? "," : "", depths[i], bitmap != NULL, first, second, count, error,
            info.header.biClrUsed, pixels[0], pixels[1], pixels[2], pixels[3]);
        for (unsigned j = 0; j < 1u << depths[i]; ++j) {
            DWORD value; memcpy(&value, &info.table[j], sizeof(value));
            printf("%s%lu", j ? "," : "", value);
        }
        printf("]}");
        DeleteDC(dc); if (bitmap) DeleteObject(bitmap);
    }
    printf("]");
}

struct PaintProbeEvent {
    LONG stage;
    UINT message;
    DWORD thread;
    BOOL dc;
    BOOL erase;
    RECT rect;
    BOOL query;
    RECT query_rect;
};

struct PaintProbeResult {
    LONG stage;
    ULONG_PTR result;
    DWORD error;
    BOOL dirty;
    BOOL erase;
    RECT rect;
};

static volatile LONG paint_probe_stage;
static volatile LONG paint_probe_count;
static struct PaintProbeEvent paint_probe_events[64];
static struct PaintProbeResult paint_probe_results[7];
static DWORD paint_probe_owner;
static DWORD paint_probe_worker;
static const RECT * volatile paint_probe_query;

static void paint_probe_event(UINT message, HDC dc, const PAINTSTRUCT *paint) {
    LONG index = InterlockedIncrement(&paint_probe_count) - 1;
    if (index < 64) {
        struct PaintProbeEvent *event = &paint_probe_events[index];
        event->stage = InterlockedCompareExchange(&paint_probe_stage, 0, 0);
        event->message = message;
        event->thread = GetCurrentThreadId();
        event->dc = dc != NULL;
        if (paint) { event->erase = paint->fErase; event->rect = paint->rcPaint; }
        const RECT *query = paint_probe_query;
        if (query) { event->query = TRUE; event->query_rect = *query; }
    }
}

static LRESULT CALLBACK paint_probe_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_ERASEBKGND) {
        paint_probe_event(message, (HDC)wparam, NULL);
        return 1;
    }
    if (message == WM_PAINT) {
        paint_probe_event(message, NULL, NULL);
        PAINTSTRUCT paint = {0};
        HDC dc = BeginPaint(window, &paint);
        paint_probe_event(WM_PAINT + 0x10000, dc, &paint);
        if (dc) EndPaint(window, &paint);
        return 0;
    }
    return DefWindowProcA(window, message, wparam, lparam);
}

static void paint_probe_call(HWND window, LONG stage, unsigned output, BOOL invalidate, BOOL erase) {
    struct PaintProbeResult *result = &paint_probe_results[output];
    InterlockedExchange(&paint_probe_stage, stage);
    result->stage = stage;
    if (invalidate) { ValidateRect(window, NULL); InvalidateRect(window, NULL, erase); }
    SetLastError(0xabcdef01);
    if (stage == 4 || stage == 10) {
        PAINTSTRUCT paint = {0};
        HDC dc = BeginPaint(window, &paint);
        result->result = dc != NULL;
        result->error = GetLastError();
        result->erase = paint.fErase;
        result->rect = paint.rcPaint;
        if (dc) EndPaint(window, &paint);
    } else if (stage == 12) {
        result->result = UpdateWindow(window);
        result->error = GetLastError();
    } else {
        result->rect = (RECT){101,102,103,104};
        paint_probe_query = &result->rect;
        result->result = GetUpdateRect(window, &result->rect, TRUE);
        result->error = GetLastError();
        paint_probe_query = NULL;
    }
    result->dirty = GetUpdateRect(window, NULL, FALSE);
}

static DWORD WINAPI paint_probe_run(void *argument) {
    HWND window = (HWND)argument;
    paint_probe_worker = GetCurrentThreadId();
    paint_probe_call(window, 10, 4, TRUE, TRUE);
    paint_probe_call(window, 11, 5, TRUE, TRUE);
    paint_probe_call(window, 12, 6, TRUE, TRUE);
    return 0;
}

static void paint_thread_reference(void) {
    WNDCLASSA info = {0};
    info.lpfnWndProc = paint_probe_proc;
    info.hInstance = GetModuleHandleA(NULL);
    info.lpszClassName = "ring3-paint-probe";
    ATOM atom = RegisterClassA(&info);
    HWND window = atom ? CreateWindowExA(0, info.lpszClassName, "paint", WS_POPUP | WS_VISIBLE,
        0, 0, 32, 24, NULL, NULL, info.hInstance, NULL) : NULL;
    paint_probe_owner = GetCurrentThreadId();
    BOOL complete = FALSE;
    DWORD wait_error = 0;
    if (window) {
        UpdateWindow(window);
        ValidateRect(window, NULL);
        memset(paint_probe_events, 0, sizeof(paint_probe_events));
        InterlockedExchange(&paint_probe_count, 0);
        paint_probe_call(window, 1, 0, TRUE, FALSE);
        paint_probe_call(window, 2, 1, TRUE, TRUE);
        paint_probe_call(window, 3, 2, FALSE, FALSE);
        paint_probe_call(window, 4, 3, FALSE, FALSE);
        HANDLE worker = CreateThread(NULL, 0, paint_probe_run, window, 0, NULL);
        if (worker) {
            DWORD start = GetTickCount();
            while (GetTickCount() - start < 5000) {
                DWORD elapsed = GetTickCount() - start;
                if (elapsed >= 5000) break;
                DWORD waited = MsgWaitForMultipleObjects(1, &worker, FALSE, 5000 - elapsed, QS_SENDMESSAGE);
                if (waited == WAIT_OBJECT_0) { complete = TRUE; break; }
                if (waited != WAIT_OBJECT_0 + 1) { wait_error = waited == WAIT_FAILED ? GetLastError() : waited; break; }
                MSG message;
                // dispatch sent calls without retrieving a queued paint.
                PeekMessageA(&message, NULL, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE);
            }
            if (!complete) {
                if (!wait_error) wait_error = WAIT_TIMEOUT;
                fprintf(stderr, "paint worker join failed: %lu\n", wait_error);
                fflush(stderr);
                ExitProcess(3);
            }
            if (WaitForSingleObject(worker, 0) != WAIT_OBJECT_0) {
                fputs("paint worker was not joined\n", stderr);
                fflush(stderr);
                ExitProcess(4);
            }
            CloseHandle(worker);
        } else {
            fprintf(stderr, "paint worker creation failed: %lu\n", GetLastError());
            fflush(stderr);
            ExitProcess(5);
        }
    } else {
        fprintf(stderr, "paint window creation failed: %lu\n", GetLastError());
        fflush(stderr);
        ExitProcess(6);
    }
    LONG count = InterlockedCompareExchange(&paint_probe_count, 0, 0);
    printf(",\"paintThreads\":{\"created\":%u,\"complete\":%u,\"waitError\":%lu,\"owner\":%lu,\"worker\":%lu,\"eventCount\":%ld,\"results\":[",
        window != NULL, complete, wait_error, paint_probe_owner, paint_probe_worker, count);
    for (unsigned i = 0; complete && i < 7; ++i) {
        const struct PaintProbeResult *result = &paint_probe_results[i];
        printf("%s{\"stage\":%ld,\"result\":%llu,\"error\":%lu,\"dirty\":%u,\"erase\":%u,\"rect\":[%ld,%ld,%ld,%ld]}",
            i ? "," : "", result->stage, (unsigned long long)result->result, result->error,
            result->dirty, result->erase, result->rect.left, result->rect.top, result->rect.right, result->rect.bottom);
    }
    printf("],\"events\":[");
    for (LONG i = 0; complete && i < count && i < 64; ++i) {
        const struct PaintProbeEvent *event = &paint_probe_events[i];
        printf("%s{\"stage\":%ld,\"message\":%u,\"thread\":%lu,\"isOwner\":%u,\"isWorker\":%u,\"dc\":%u,\"erase\":%u,\"rect\":[%ld,%ld,%ld,%ld],\"query\":%u,\"queryRect\":[%ld,%ld,%ld,%ld]}",
            i ? "," : "", event->stage, event->message, event->thread,
            event->thread == paint_probe_owner, event->thread == paint_probe_worker, event->dc,
            event->erase, event->rect.left, event->rect.top, event->rect.right, event->rect.bottom,
            event->query, event->query_rect.left, event->query_rect.top, event->query_rect.right, event->query_rect.bottom);
    }
    printf("]}");
    DestroyWindow(window);
    UnregisterClassA(info.lpszClassName, info.hInstance);
}

static void stretch_mode_reference(void) {
    struct MonoInfo { BITMAPINFOHEADER header; RGBQUAD colors[2]; } mono = {0};
    BITMAPINFO rgb = {0};
    mono.header.biSize = sizeof(mono.header);
    mono.header.biWidth = 4; mono.header.biHeight = -1;
    mono.header.biPlanes = 1; mono.header.biBitCount = 1;
    mono.colors[1].rgbRed = mono.colors[1].rgbGreen = mono.colors[1].rgbBlue = 255;
    rgb.bmiHeader.biSize = sizeof(rgb.bmiHeader);
    rgb.bmiHeader.biWidth = 4; rgb.bmiHeader.biHeight = -4;
    rgb.bmiHeader.biPlanes = 1; rgb.bmiHeader.biBitCount = 32;
    printf(",\"stretchModes\":[");
    for (unsigned type = 0; type < 2; ++type) {
        for (int mode = 1; mode <= 4; ++mode) {
            void *source_bits = NULL, *target_bits = NULL;
            BITMAPINFO *info = type ? &rgb : (BITMAPINFO *)&mono;
            HBITMAP source = CreateDIBSection(NULL, info, DIB_RGB_COLORS, &source_bits, NULL, 0);
            HBITMAP target = CreateDIBSection(NULL, info, DIB_RGB_COLORS, &target_bits, NULL, 0);
            HDC source_dc = CreateCompatibleDC(NULL), target_dc = CreateCompatibleDC(NULL);
            HGDIOBJ old_source = SelectObject(source_dc, source), old_target = SelectObject(target_dc, target);
            if (source_bits && target_bits) {
                if (!type) { memset(source_bits, 0, 4); *(BYTE *)source_bits = 0x50; memset(target_bits, 0x5a, 4); }
                else {
                    DWORD *pixels = (DWORD *)source_bits;
                    const DWORD colors[] = {0xff0000, 0x00ff00, 0x0000ff, 0xffffff};
                    for (unsigned y = 0; y < 4; ++y)
                        for (unsigned x = 0; x < 4; ++x) pixels[y * 4 + x] = colors[(x + y) % 4];
                    memset(target_bits, 0x5a, 64);
                }
            }
            int previous = SetStretchBltMode(target_dc, mode);
            SetBrushOrgEx(target_dc, 0, 0, NULL);
            SetLastError(0xabcdef01);
            BOOL result = StretchBlt(target_dc, 0, 0, type ? 3 : 2, type ? 3 : 1,
                source_dc, 0, 0, 4, type ? 4 : 1, SRCCOPY);
            DWORD error = GetLastError();
            GdiFlush();
            printf("%s{\"bits\":%u,\"mode\":%d,\"created\":%u,\"selected\":%u,\"previous\":%d,\"result\":%u,\"error\":%lu,\"pixels\":[",
                type || mode != 1 ? "," : "", type ? 32 : 1, mode,
                source && target, old_source && old_source != HGDI_ERROR && old_target && old_target != HGDI_ERROR,
                previous, result, error);
            unsigned count = type ? 16 : 4;
            for (unsigned i = 0; i < count; ++i)
                printf("%s%lu", i ? "," : "", target_bits ? (type ? ((DWORD *)target_bits)[i] : ((BYTE *)target_bits)[i]) : 0);
            printf("]}");
            SelectObject(source_dc, old_source); SelectObject(target_dc, old_target);
            DeleteDC(source_dc); DeleteDC(target_dc); DeleteObject(source); DeleteObject(target);
        }
    }
    printf("]");
}

static void rotated_font_reference(void) {
    BITMAPINFO info = {0};
    info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = 128; info.bmiHeader.biHeight = -128;
    info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 32;
    const int angles[] = {0,900,-900,450,3600,0};
    const int orientations[] = {0,0,-900,450,0,900};
    printf(",\"rotatedFonts\":[");
    for (unsigned i = 0; i < sizeof(angles)/sizeof(angles[0]); ++i) {
        void *bits = NULL;
        HBITMAP bitmap = CreateDIBSection(NULL, &info, DIB_RGB_COLORS, &bits, NULL, 0);
        HDC dc = CreateCompatibleDC(NULL);
        HGDIOBJ old_bitmap = SelectObject(dc, bitmap);
        HFONT font = CreateFontW(-16, 0, angles[i], orientations[i], FW_NORMAL, FALSE, FALSE, FALSE,
            ANSI_CHARSET, OUT_TT_PRECIS, CLIP_DEFAULT_PRECIS, NONANTIALIASED_QUALITY, DEFAULT_PITCH, L"Arial");
        HGDIOBJ old_font = SelectObject(dc, font);
        if (bits) for (unsigned j = 0; j < 128 * 128; ++j) ((DWORD *)bits)[j] = 0xffffff;
        SetTextColor(dc, 0); SetBkMode(dc, TRANSPARENT);
        SetTextAlign(dc, TA_BASELINE | TA_UPDATECP);
        MoveToEx(dc, 64, 64, NULL);
        SIZE extent = {0}; POINT position = {0};
        GetTextExtentPoint32W(dc, L"Mm", 2, &extent);
        SetLastError(0xabcdef01);
        BOOL result = TextOutW(dc, 0, 0, L"Mm", 2);
        DWORD error = GetLastError();
        GetCurrentPositionEx(dc, &position); GdiFlush();
        RECT ink = {128,128,0,0}; unsigned count = 0;
        if (bits) for (int y = 0; y < 128; ++y) for (int x = 0; x < 128; ++x) {
            if ((((DWORD *)bits)[y * 128 + x] & 0xffffff) != 0xffffff) {
                if (x < ink.left) ink.left = x; if (y < ink.top) ink.top = y;
                if (x + 1 > ink.right) ink.right = x + 1; if (y + 1 > ink.bottom) ink.bottom = y + 1;
                ++count;
            }
        }
        WCHAR face[32] = {0}; GetTextFaceW(dc, 32, face);
        printf("%s{\"escapement\":%d,\"orientation\":%d,\"created\":%u,\"selected\":%u,\"mode\":%d,\"result\":%u,\"error\":%lu,\"extent\":[%ld,%ld],\"position\":[%ld,%ld],\"ink\":[%ld,%ld,%ld,%ld],\"inkPixels\":%u,\"face\":[",
            i ? "," : "", angles[i], orientations[i], bitmap && font,
            old_bitmap && old_bitmap != HGDI_ERROR && old_font && old_font != HGDI_ERROR,
            GetGraphicsMode(dc), result, error, extent.cx, extent.cy, position.x, position.y,
            ink.left, ink.top, ink.right, ink.bottom, count);
        for (unsigned j = 0; face[j]; ++j) printf("%s%u", j ? "," : "", face[j]);
        printf("]}");
        SelectObject(dc, old_font); SelectObject(dc, old_bitmap);
        DeleteObject(font); DeleteDC(dc); DeleteObject(bitmap);
    }
    printf("]");
}


struct font_probe_state { unsigned count, stop; int value; WCHAR first[LF_FACESIZE]; BYTE charset; DWORD kind; };
static int CALLBACK font_probe(const LOGFONTW *font, const TEXTMETRICW *metrics, DWORD kind, LPARAM parameter) {
    struct font_probe_state *state = (struct font_probe_state *)parameter;
    if (!state->count) {
        memcpy(state->first, font->lfFaceName, sizeof(state->first));
        state->charset = metrics->tmCharSet;
        state->kind = kind;
    }
    ++state->count;
    return state->stop && state->count >= state->stop ? 0 : state->value;
}
static void font_probe_case(HDC dc, const char *name, const WCHAR *family, BYTE charset, unsigned stop, int value, int comma) {
    LOGFONTW filter = {0}; filter.lfCharSet = charset;
    if (family) lstrcpynW(filter.lfFaceName, family, LF_FACESIZE);
    struct font_probe_state state = {0}; state.stop = stop; state.value = value;
    SetLastError(0xabcdef01);
    int result = EnumFontFamiliesExW(dc, &filter, font_probe, (LPARAM)&state, 0);
    DWORD error = GetLastError();
    printf("%s{\"case\":\"%s\",\"result\":%d,\"calls\":%u,\"charset\":%u,\"kind\":%lu,\"error\":%lu,\"first\":[", comma ? "," : "", name, result, state.count, state.charset, state.kind, error);
    for (unsigned i = 0; i < LF_FACESIZE && state.first[i]; ++i) printf("%s%u", i ? "," : "", state.first[i]);
    printf("]}");
}
static void font_resource_reference(void) {
    HDC dc = CreateCompatibleDC(NULL);
    WCHAR system_face[LF_FACESIZE] = {0}; GetTextFaceW(dc, LF_FACESIZE, system_face);
    printf(",\"fontEnumeration\":[");
    font_probe_case(dc, "absent", L"ring3 absent font family", DEFAULT_CHARSET, 0, 42, 0);
    font_probe_case(dc, "last42", system_face, ANSI_CHARSET, 0, 42, 1);
    font_probe_case(dc, "stop0", NULL, DEFAULT_CHARSET, 1, 42, 1);
    font_probe_case(dc, "beforeMemory", L"Noto Sans", DEFAULT_CHARSET, 0, 42, 1);
    printf("]");
    FILE *input = fopen("engine/assets/fonts/NotoSans-Regular.ttf", "rb");
    BYTE *bytes = NULL; long length = 0;
    if (input && !fseek(input, 0, SEEK_END)) {
        length = ftell(input);
        if (length > 0 && length <= 4 * 1024 * 1024 && !fseek(input, 0, SEEK_SET)) {
            bytes = malloc((size_t)length);
            if (bytes && fread(bytes, 1, (size_t)length, input) != (size_t)length) { free(bytes); bytes = NULL; }
        }
    }
    if (input) fclose(input);
    DWORD count = 0x12345678;
    SetLastError(0xabcdef01);
    HANDLE memory_font = bytes ? AddFontMemResourceEx(bytes, (DWORD)length, NULL, &count) : NULL;
    DWORD add_error = GetLastError(); free(bytes);
    HFONT font = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, ANSI_CHARSET, OUT_TT_PRECIS, CLIP_DEFAULT_PRECIS, DEFAULT_QUALITY, DEFAULT_PITCH, L"Noto Sans");
    HGDIOBJ old_font = SelectObject(dc, font);
    WCHAR before[LF_FACESIZE] = {0}, after[LF_FACESIZE] = {0};
    SIZE before_size = {0}, after_size = {0};
    GetTextFaceW(dc, LF_FACESIZE, before); GetTextExtentPoint32W(dc, L"Mm", 2, &before_size);
    printf(",\"fontMemoryEnumeration\":[");
    font_probe_case(dc, "afterMemory", L"Noto Sans", DEFAULT_CHARSET, 0, 42, 0);
    printf("]");
    SetLastError(0xabcdef01);
    BOOL removed = memory_font ? RemoveFontMemResourceEx(memory_font) : FALSE;
    DWORD remove_error = GetLastError();
    GetTextFaceW(dc, LF_FACESIZE, after); GetTextExtentPoint32W(dc, L"Mm", 2, &after_size);
    printf(",\"fontMemory\":{\"input\":%u,\"added\":%u,\"count\":%lu,\"addError\":%lu,\"removed\":%u,\"removeError\":%lu,\"sameSelectedFace\":%u,\"beforeWidth\":%ld,\"afterWidth\":%ld}",
        length > 0, memory_font != NULL, count, add_error, removed, remove_error, !lstrcmpW(before, after), before_size.cx, after_size.cx);
    SelectObject(dc, old_font); DeleteObject(font);
    int file_first = AddFontResourceExW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", FR_PRIVATE, NULL);
    int file_second = AddFontResourceExW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", FR_PRIVATE, NULL);
    printf(",\"fontFileEnumeration\":[");
    font_probe_case(dc, "afterFile", L"Noto Sans", DEFAULT_CHARSET, 0, 42, 0);
    printf("]");
    BOOL wrong_flags = RemoveFontResourceExW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", 0, NULL);
    BOOL remove_first = RemoveFontResourceExW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", FR_PRIVATE, NULL);
    BOOL remove_second = RemoveFontResourceExW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", FR_PRIVATE, NULL);
    BOOL remove_third = RemoveFontResourceExW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", FR_PRIVATE, NULL);
    printf(",\"fontFile\":{\"first\":%d,\"second\":%d,\"wrongFlags\":%u,\"removeFirst\":%u,\"removeSecond\":%u,\"removeThird\":%u}",file_first,file_second,wrong_flags,remove_first,remove_second,remove_third);
    DeleteDC(dc);
}


static void font_file_access(const WCHAR *path, const char *stage) {
    const DWORD accesses[] = {GENERIC_READ, GENERIC_WRITE, DELETE};
    printf("{\"stage\":\"%s\",\"access\":[", stage);
    for (unsigned i = 0; i < 3; ++i) {
        SetLastError(0);
        HANDLE file = CreateFileW(path, accesses[i], FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
        DWORD error = GetLastError();
        printf("%s{\"mask\":%lu,\"opened\":%u,\"error\":%lu}", i ? "," : "", accesses[i], file != INVALID_HANDLE_VALUE, error);
        if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    }
    printf("]}");
}

static void font_file_sharing_reference(void) {
    WCHAR directory[MAX_PATH], path[MAX_PATH];
    if (!GetTempPathW(MAX_PATH, directory) || !GetTempFileNameW(directory, L"r3f", 0, path)
        || lstrlenW(path) > MAX_PATH - 5 || !DeleteFileW(path)) ExitProcess(2);
    lstrcatW(path, L".ttf");
    if (!CopyFileW(L"engine\\assets\\fonts\\NotoSans-Regular.ttf", path, TRUE)) ExitProcess(2);
    printf(",\"fontFileSharing\":{\"states\":[");
    font_file_access(path, "before-add");
    int added = AddFontResourceExW(path, FR_PRIVATE, NULL);
    printf(","); font_file_access(path, "after-add");
    BOOL removed = added ? RemoveFontResourceExW(path, FR_PRIVATE, NULL) : FALSE;
    printf(","); font_file_access(path, "after-remove");
    BOOL deleted = DeleteFileW(path);
    printf("],\"added\":%d,\"removed\":%u,\"deleted\":%u}", added, removed, deleted);
    if (!added) { fflush(stdout); ExitProcess(2); }
}

static void font_face_units(const WCHAR *name) {
    printf("[");
    for (unsigned i = 0; i < LF_FACESIZE && name[i]; ++i) printf("%s%u", i ? "," : "", name[i]);
    printf("]");
}
static void font_selection_state(HDC dc, HFONT font, DWORD *pixels, const char *label, int comma) {
    WCHAR before[LF_FACESIZE] = {0}, after[LF_FACESIZE] = {0};
    TEXTMETRICW metrics = {0}; SIZE extent = {0}; LOGFONTW logical = {0};
    int before_count = GetTextFaceW(dc, LF_FACESIZE, before);
    BOOL metric_result = GetTextMetricsW(dc, &metrics);
    BOOL extent_result = GetTextExtentPoint32W(dc, L"MmW", 3, &extent);
    int object_size = GetObjectW(font, sizeof(logical), &logical);
    BYTE header[16] = {0}; DWORD data_size = GetFontData(dc, 0, 0, header, sizeof(header));
    DWORD hash = 2166136261u; unsigned ink = 0;
    if (pixels) for (unsigned i = 0; i < 96 * 48; ++i) pixels[i] = 0x00ffffff;
    SetBkMode(dc, TRANSPARENT); SetTextColor(dc, RGB(0,0,0));
    BOOL drawn = TextOutW(dc, 4, 4, L"MmW", 3);
    GdiFlush();
    if (pixels) for (unsigned i = 0; i < 96 * 48; ++i) {
        DWORD color = pixels[i] & 0x00ffffff;
        if (color != 0x00ffffff) ++ink;
        hash = (hash ^ color) * 16777619u;
    }
    int after_count = GetTextFaceW(dc, LF_FACESIZE, after);
    printf("%s{\"stage\":\"%s\",\"currentHandle\":%u,\"beforeCount\":%d,\"beforeFace\":", comma ? "," : "", label,
        GetCurrentObject(dc, OBJ_FONT) == font, before_count);
    font_face_units(before);
    printf(",\"afterCount\":%d,\"afterFace\":", after_count); font_face_units(after);
    printf(",\"metricResult\":%u,\"metrics\":[%ld,%ld,%ld,%ld,%ld,%ld,%u,%u],\"extentResult\":%u,\"extent\":[%ld,%ld],\"objectBytes\":%d,\"logicalFace\":",
        metric_result, metrics.tmHeight, metrics.tmAscent, metrics.tmDescent, metrics.tmAveCharWidth,
        metrics.tmMaxCharWidth, metrics.tmWeight, metrics.tmPitchAndFamily, metrics.tmCharSet,
        extent_result, extent.cx, extent.cy, object_size);
    font_face_units(logical.lfFaceName);
    printf(",\"fontDataBytes\":%lu,\"fontData\":[", data_size);
    for (unsigned i = 0; i < sizeof(header); ++i) printf("%s%u", i ? "," : "", header[i]);
    printf("],\"drawn\":%u,\"ink\":%u,\"hash\":%lu}", drawn, ink, hash);
}
static void font_lifetime_reference(void) {
    FILE *input = fopen("engine/assets/fonts/NotoSans-Regular.ttf", "rb");
    BYTE *bytes = NULL; long length = 0;
    if (input && !fseek(input, 0, SEEK_END)) {
        length = ftell(input);
        if (length > 0 && length <= 4 * 1024 * 1024 && !fseek(input, 0, SEEK_SET)) {
            bytes = malloc((size_t)length);
            if (bytes && fread(bytes, 1, (size_t)length, input) != (size_t)length) { free(bytes); bytes = NULL; }
        }
    }
    if (input) fclose(input);
    printf(",\"fontSelectionLifetime\":[");
    for (unsigned phase = 0; phase < 3; ++phase) {
        HDC dc = CreateCompatibleDC(NULL);
        BITMAPINFO info = {0}; info.bmiHeader.biSize = sizeof(info.bmiHeader);
        info.bmiHeader.biWidth = 96; info.bmiHeader.biHeight = -48;
        info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 32;
        DWORD *pixels = NULL;
        HBITMAP bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, (void **)&pixels, NULL, 0);
        HGDIOBJ old_bitmap = SelectObject(dc, bitmap);
        HFONT font = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, ANSI_CHARSET, OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS, DEFAULT_QUALITY, DEFAULT_PITCH, L"Noto Sans");
        HGDIOBJ old_font = phase == 2 ? SelectObject(dc, font) : GetCurrentObject(dc, OBJ_FONT);
        printf("%s{\"case\":\"%s\",\"created\":%u,\"states\":[", phase ? "," : "",
            phase == 0 ? "create-before-add-selected-remove" : phase == 1 ? "remove-before-first-select" : "reselect-after-add", font != NULL);
        if (phase == 2) font_selection_state(dc, font, pixels, "selected-before-add", 0);
        DWORD count = 0; HANDLE resource = bytes ? AddFontMemResourceEx(bytes, (DWORD)length, NULL, &count) : NULL;
        BOOL removed = FALSE;
        if (phase == 1) {
            removed = resource ? RemoveFontMemResourceEx(resource) : FALSE;
            SelectObject(dc, font);
            font_selection_state(dc, font, pixels, "first-select-after-remove", 0);
        } else {
            SelectObject(dc, font);
            font_selection_state(dc, font, pixels, "selected-after-add", phase == 2);
            int saved = SaveDC(dc);
            removed = resource ? RemoveFontMemResourceEx(resource) : FALSE;
            font_selection_state(dc, font, pixels, "selected-after-remove", 1);
            SelectObject(dc, old_font);
            BOOL restored = saved ? RestoreDC(dc, saved) : FALSE;
            font_selection_state(dc, font, pixels, restored ? "restored-after-remove" : "restore-failed", 1);
            SelectObject(dc, old_font); SelectObject(dc, font);
            font_selection_state(dc, font, pixels, "reselected-after-remove", 1);
        }
        printf("],\"added\":%u,\"count\":%lu,\"removed\":%u}", resource != NULL, count, removed);
        SelectObject(dc, old_font); SelectObject(dc, old_bitmap);
        DeleteObject(font); DeleteObject(bitmap); DeleteDC(dc);
    }
    printf("]"); free(bytes);
}


static void font_scope_reference(void) {
    FILE *input = fopen("engine/assets/fonts/NotoSans-Regular.ttf", "rb");
    BYTE *bytes = NULL; long length = 0;
    if (input && !fseek(input, 0, SEEK_END)) {
        length = ftell(input);
        if (length > 0 && length <= 4 * 1024 * 1024 && !fseek(input, 0, SEEK_SET)) {
            bytes = malloc((size_t)length);
            if (bytes && fread(bytes, 1, (size_t)length, input) != (size_t)length) { free(bytes); bytes = NULL; }
        }
    }
    if (input) fclose(input);
    HDC dc[3]; HBITMAP bitmap[3]; HGDIOBJ old_bitmap[3], old_font[3]; DWORD *pixels[3] = {0};
    BITMAPINFO info = {0}; info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = 96; info.bmiHeader.biHeight = -48;
    info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 32;
    for (unsigned i = 0; i < 3; ++i) {
        dc[i] = CreateCompatibleDC(NULL);
        bitmap[i] = CreateDIBSection(dc[i], &info, DIB_RGB_COLORS, (void **)&pixels[i], NULL, 0);
        old_bitmap[i] = SelectObject(dc[i], bitmap[i]); old_font[i] = GetCurrentObject(dc[i], OBJ_FONT);
    }
    HFONT first = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, ANSI_CHARSET, OUT_TT_PRECIS,
        CLIP_DEFAULT_PRECIS, DEFAULT_QUALITY, DEFAULT_PITCH, L"Noto Sans");
    SelectObject(dc[0], first);
    printf(",\"fontRealizationScope\":{\"states\":[");
    font_selection_state(dc[0], first, pixels[0], "first-font-dc0-before-add", 0);
    DWORD count = 0; HANDLE resource = bytes ? AddFontMemResourceEx(bytes, (DWORD)length, NULL, &count) : NULL;
    free(bytes);
    SelectObject(dc[1], first);
    font_selection_state(dc[1], first, pixels[1], "first-font-dc1-first-select-after-add", 1);
    HFONT second = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, ANSI_CHARSET, OUT_TT_PRECIS,
        CLIP_DEFAULT_PRECIS, DEFAULT_QUALITY, DEFAULT_PITCH, L"Noto Sans");
    SelectObject(dc[0], second);
    font_selection_state(dc[0], second, pixels[0], "second-font-dc0-first-select-after-add", 1);
    SelectObject(dc[1], second);
    font_selection_state(dc[1], second, pixels[1], "second-font-dc1-first-select-after-add", 1);
    BOOL removed = resource ? RemoveFontMemResourceEx(resource) : FALSE;
    font_selection_state(dc[0], second, pixels[0], "second-font-dc0-selected-after-remove", 1);
    SelectObject(dc[2], second);
    font_selection_state(dc[2], second, pixels[2], "second-font-dc2-first-select-after-remove", 1);
    for (unsigned i = 0; i < 3; ++i) SelectObject(dc[i], old_font[i]);
    SelectObject(dc[0], second);
    font_selection_state(dc[0], second, pixels[0], "second-font-dc0-after-all-deselected", 1);
    HFONT third = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, ANSI_CHARSET, OUT_TT_PRECIS,
        CLIP_DEFAULT_PRECIS, DEFAULT_QUALITY, DEFAULT_PITCH, L"Noto Sans");
    SelectObject(dc[1], third);
    font_selection_state(dc[1], third, pixels[1], "third-font-dc1-created-after-remove", 1);
    printf("],\"added\":%u,\"removed\":%u,\"count\":%lu}", resource != NULL, removed, count);
    for (unsigned i = 0; i < 3; ++i) {
        SelectObject(dc[i], old_font[i]); SelectObject(dc[i], old_bitmap[i]);
        DeleteObject(bitmap[i]); DeleteDC(dc[i]);
    }
    DeleteObject(first); DeleteObject(second); DeleteObject(third);
}


static void msimg_reference(void) {
    BITMAPINFO info = {0}; info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = 4; info.bmiHeader.biHeight = -2;
    info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 32;
    DWORD *source_pixels = NULL, *target_pixels = NULL;
    HDC source = CreateCompatibleDC(NULL), target = CreateCompatibleDC(NULL);
    HBITMAP source_bitmap = CreateDIBSection(source, &info, DIB_RGB_COLORS, (void **)&source_pixels, NULL, 0);
    HBITMAP target_bitmap = CreateDIBSection(target, &info, DIB_RGB_COLORS, (void **)&target_pixels, NULL, 0);
    HGDIOBJ old_source = SelectObject(source, source_bitmap), old_target = SelectObject(target, target_bitmap);
    printf(",\"msimg\":{\"alpha\":[");
    const BYTE constants[] = {255,128,128,255,255};
    const BYTE formats[] = {AC_SRC_ALPHA,AC_SRC_ALPHA,0,0,AC_SRC_ALPHA};
    const DWORD inputs[] = {0x80400000,0x80400000,0x80400000,0x80400000,0x80800000};
    for (unsigned i = 0; i < 5; ++i) {
        if (source_pixels) for (unsigned p = 0; p < 8; ++p) source_pixels[p] = inputs[i];
        if (target_pixels) for (unsigned p = 0; p < 8; ++p) target_pixels[p] = 0x40000040;
        BLENDFUNCTION blend = {AC_SRC_OVER, 0, constants[i], formats[i]};
        SetLastError(0xabcdef01);
        BOOL result = AlphaBlend(target, 0, 0, 2, 1, source, 0, 0, 4, 2, blend);
        DWORD error = GetLastError(); GdiFlush();
        printf("%s{\"source\":%lu,\"destination\":1073741888,\"constant\":%u,\"format\":%u,\"result\":%u,\"error\":%lu,\"pixels\":[%lu,%lu,%lu]}",
            i ? "," : "", inputs[i], constants[i], formats[i], result, error,
            target_pixels ? target_pixels[0] : 0, target_pixels ? target_pixels[1] : 0, target_pixels ? target_pixels[2] : 0);
    }
    if (source_pixels) { source_pixels[0] = 0x80400000; source_pixels[1] = 0x9900ff00; }
    if (target_pixels) for (unsigned p = 0; p < 8; ++p) target_pixels[p] = 0x40000040;
    SetLastError(0xabcdef01);
    BOOL transparent = TransparentBlt(target, 0, 0, 2, 1, source, 0, 0, 2, 1, RGB(64,0,0));
    DWORD transparent_error = GetLastError(); GdiFlush();
    printf("],\"transparent\":{\"result\":%u,\"error\":%lu,\"pixels\":[%lu,%lu,%lu]},\"gradients\":[", transparent, transparent_error,
        target_pixels ? target_pixels[0] : 0, target_pixels ? target_pixels[1] : 0, target_pixels ? target_pixels[2] : 0);
    for (unsigned mode = 0; mode <= 2; ++mode) {
        if (target_pixels) for (unsigned p = 0; p < 8; ++p) target_pixels[p] = 0x5a123456;
        TRIVERTEX vertices[3] = {{0,0,0xff00,0,0,0xffff}, {4,2,0,0,0xff00,0xffff}, {4,0,0,0xff00,0,0xffff}};
        GRADIENT_RECT rect = {0,1}; GRADIENT_TRIANGLE triangle = {0,2,1};
        SetLastError(0xabcdef01);
        BOOL result = GradientFill(target, vertices, 3, mode == GRADIENT_FILL_TRIANGLE ? (void *)&triangle : (void *)&rect, 1, mode);
        DWORD error = GetLastError(); GdiFlush();
        printf("%s{\"mode\":%u,\"result\":%u,\"error\":%lu,\"pixels\":[", mode ? "," : "", mode, result, error);
        for (unsigned p = 0; p < 8; ++p) printf("%s%lu", p ? "," : "", target_pixels ? target_pixels[p] : 0);
        printf("]}");
    }
    printf("]}");
    SelectObject(source, old_source); SelectObject(target, old_target);
    DeleteObject(source_bitmap); DeleteObject(target_bitmap); DeleteDC(source); DeleteDC(target);
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
    synthesized_palette_reference();
    indexed_ddb_reference();
    paint_thread_reference();
    stretch_mode_reference();
    rotated_font_reference();
    font_resource_reference();
    font_lifetime_reference();
    font_scope_reference();
    font_file_sharing_reference();
    msimg_reference();
    DestroyWindow(parent);
    puts("}");
    return 0;
}
