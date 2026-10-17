#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>

static HANDLE trace;
static unsigned scenario, depth;
static HFONT fonts[2];
static union { DWORD alignment; BYTE bytes[2048]; } templates[2];

static void record(DWORD event, DWORD message, DWORD value) {
    DWORD row[4] = {event, scenario | (depth << 8), message, value};
    DWORD written = 0;
    if (!WriteFile(trace, row, sizeof(row), &written, NULL) || written != sizeof(row)) ExitProcess(250);
}

static void require(BOOL condition, DWORD code) {
    if (!condition) { record('!', code, GetLastError()); ExitProcess(code); }
}

static BYTE *word(BYTE *p, WORD value) {
    p[0] = (BYTE)value; p[1] = (BYTE)(value >> 8); return p + 2;
}
static BYTE *dword(BYTE *p, DWORD value) {
    p = word(p, (WORD)value); return word(p, (WORD)(value >> 16));
}
static BYTE *text(BYTE *p, const WCHAR *value) {
    do { p = word(p, *value); } while (*value++);
    return p;
}

static void make(BYTE *bytes, BOOL extended, BOOL wide) {
    volatile BYTE *clear = bytes;
    for (unsigned i = 0; i != 2048; ++i) clear[i] = 0;
    BYTE *p = bytes;
    if (extended) {
        p = word(p, 1); p = word(p, 0xffff);
        p = dword(p, 0x11223344); p = dword(p, 0);
        p = dword(p, WS_POPUP | DS_SETFONT);
    } else {
        p = dword(p, WS_POPUP | DS_SETFONT); p = dword(p, 0);
    }
    p = word(p, 1); p = word(p, 0); p = word(p, 0);
    p = word(p, 100); p = word(p, 60);
    p = word(p, 0); p = word(p, 0); p = text(p, L"authored dialog");
    p = word(p, 9);
    if (extended) { p = word(p, 700); p = word(p, 0x0101); }
    p = text(p, L"Arial");
    p = (BYTE *)(((uintptr_t)p + 3) & ~(uintptr_t)3);
    if (extended) { p = dword(p, 0x55667788); p = dword(p, 0); p = dword(p, WS_CHILD | WS_VISIBLE | WS_TABSTOP); }
    else { p = dword(p, WS_CHILD | WS_VISIBLE | WS_TABSTOP); p = dword(p, 0); }
    p = word(p, 4); p = word(p, 8); p = word(p, 20); p = word(p, 12);
    if (extended) p = dword(p, 100); else p = word(p, 100);
    p = text(p, wide ? L"R3DialogChildW" : L"R3DialogChildA");
    p = text(p, L"child");
    p = word(p, extended ? 4 : 6); p = word(p, 0x1234); p = word(p, 0x5678);
    (void)p;
}

static LRESULT CALLBACK child(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    BOOL wide = (scenario & 2) != 0;
    if (message == WM_CREATE) {
        const CREATESTRUCTW *create = (const CREATESTRUCTW *)lparam;
        const WORD *data = (const WORD *)create->lpCreateParams;
        require(data && data[0] == ((scenario & 1) ? 4 : 6) && data[1] == 0x1234 && data[2] == 0x5678, 10);
        record('c', message, data[0]);
    } else if (message == WM_SETFONT) {
        LOGFONTW font;
        fonts[depth] = (HFONT)wparam;
        require(GetObjectW(fonts[depth], sizeof(font), &font) == sizeof(font), 11);
        require(font.lfHeight == -12 && font.lfWeight == 400 && font.lfItalic == (scenario & 1) && font.lfCharSet == 0, 12);
        record('f', message, (DWORD)font.lfHeight);
        return 0;
    } else if (message == WM_GETFONT) {
        return (LRESULT)fonts[depth];
    } else if (message == WM_DESTROY || message == WM_NCDESTROY) {
        record('x', message, (DWORD)GetDlgCtrlID(hwnd));
    }
    return wide ? DefWindowProcW(hwnd, message, wparam, lparam) : DefWindowProcA(hwnd, message, wparam, lparam);
}

static INT_PTR CALLBACK dialog(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    BOOL wide = (scenario & 2) != 0;
    if (message == WM_INITDIALOG) {
        require((DWORD)lparam == (depth ? 0x11223344 : 0xaabbccdd), 20);
        HWND control = GetDlgItem(hwnd, 100);
        require(control != NULL && GetDlgCtrlID(control) == 100, 21);
        RECT units = {0, 0, 4, 8};
        require(MapDialogRect(hwnd, &units) && units.right > 0 && units.bottom > 0, 22);
        HDC dc = CreateCompatibleDC(NULL);
        require(dc != NULL, 23);
        HGDIOBJ old = SelectObject(dc, fonts[depth]);
        require(old != NULL && old != HGDI_ERROR, 24);
        SIZE extent;
        TEXTMETRICW metric;
        require(GetTextExtentPoint32W(dc, L"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz", 52, &extent), 25);
        require(GetTextMetricsW(dc, &metric), 26);
        require(units.right == (extent.cx / 26 + 1) / 2 && units.bottom == metric.tmHeight, 27);
        require(SelectObject(dc, old) != HGDI_ERROR && DeleteDC(dc), 28);
        record('i', message, (DWORD)lparam);
        if (scenario == 7 && depth == 0) {
            depth = 1;
            make(templates[1].bytes, TRUE, TRUE);
            HWND nested = CreateDialogIndirectParamW(GetModuleHandleW(NULL), (DLGTEMPLATE *)templates[1].bytes,
                                                     hwnd, dialog, 0x11223344);
            require(nested != NULL && DestroyWindow(nested), 29);
            require(GetDlgItem(hwnd, 100) == control, 30);
            depth = 0;
            record('n', 0, 1);
        }
        if ((scenario & 4) && depth == 0) {
            require(wide ? PostMessageW(hwnd, WM_COMMAND, 1, 0) : PostMessageA(hwnd, WM_COMMAND, 1, 0), 31);
        }
        return TRUE;
    }
    if (message == WM_COMMAND && LOWORD(wparam) == 1) {
        require(EndDialog(hwnd, 0x12345678), 32);
        record('e', message, 0x12345678);
        return TRUE;
    }
    if (message == WM_DESTROY || message == WM_NCDESTROY) record('d', message, 0);
    return FALSE;
}

void entry(void) {
    trace = CreateFileA("C:\\Temp\\dialogs.log", GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (trace == INVALID_HANDLE_VALUE) ExitProcess(240);
    WNDCLASSW unicode;
    WNDCLASSA ansi;
    volatile BYTE *p = (volatile BYTE *)&unicode;
    for (unsigned i = 0; i != sizeof(unicode); ++i) p[i] = 0;
    p = (volatile BYTE *)&ansi;
    for (unsigned i = 0; i != sizeof(ansi); ++i) p[i] = 0;
    unicode.hInstance = GetModuleHandleW(NULL);
    unicode.lpfnWndProc = child; unicode.lpszClassName = L"R3DialogChildW";
    ansi.hInstance = unicode.hInstance;
    ansi.lpfnWndProc = child; ansi.lpszClassName = "R3DialogChildA";
    require(RegisterClassW(&unicode) && RegisterClassA(&ansi), 40);
    for (scenario = 0; scenario != 8; ++scenario) {
        fonts[0] = NULL;
        make(templates[0].bytes, (scenario & 1) != 0, (scenario & 2) != 0);
        if (scenario & 4) {
            INT_PTR value = scenario & 2
                ? DialogBoxIndirectParamW(unicode.hInstance, (DLGTEMPLATE *)templates[0].bytes, NULL, dialog, 0xaabbccdd)
                : DialogBoxIndirectParamA(ansi.hInstance, (DLGTEMPLATE *)templates[0].bytes, NULL, dialog, 0xaabbccdd);
            require(value == 0x12345678, 41);
            record('r', 0, (DWORD)value);
        } else {
            HWND hwnd = scenario & 2
                ? CreateDialogIndirectParamW(unicode.hInstance, (DLGTEMPLATE *)templates[0].bytes, NULL, dialog, 0xaabbccdd)
                : CreateDialogIndirectParamA(ansi.hInstance, (DLGTEMPLATE *)templates[0].bytes, NULL, dialog, 0xaabbccdd);
            require(hwnd != NULL && DestroyWindow(hwnd), 42);
            record('r', 0, 1);
        }
        LOGFONTW deleted;
        require(GetObjectW(fonts[0], sizeof(deleted), &deleted) == 0, 43);
    }
    require(UnregisterClassW(unicode.lpszClassName, unicode.hInstance) && UnregisterClassA(ansi.lpszClassName, ansi.hInstance), 44);
    record('z', 0, 0);
    require(CloseHandle(trace), 45);
    ExitProcess(0);
}
