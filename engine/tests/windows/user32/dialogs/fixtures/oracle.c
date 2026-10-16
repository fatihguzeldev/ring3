#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static unsigned extended_mode, modal_mode;
static HFONT child_font;
static BYTE *word(BYTE *p, WORD value) { memcpy(p, &value, 2); return p + 2; }
static BYTE *dword(BYTE *p, DWORD value) { memcpy(p, &value, 4); return p + 4; }
static BYTE *text(BYTE *p, const WCHAR *value) {
    do { p = word(p, *value); } while (*value++);
    return p;
}
static BYTE *align4(BYTE *p) { return (BYTE *)(((uintptr_t)p + 3) & ~(uintptr_t)3); }

static void metrics(HFONT font) {
    LOGFONTW info;
    memset(&info, 0, sizeof(info));
    int got = GetObjectW(font, sizeof(info), &info);
    printf("{\"operation\":\"dialog-font\",\"extended\":%u,\"modal\":%u,\"got\":%d,\"height\":%ld,\"weight\":%ld,\"italic\":%u,\"charset\":%u}\n",
           extended_mode, modal_mode, got, (long)info.lfHeight, (long)info.lfWeight,
           (unsigned)info.lfItalic, (unsigned)info.lfCharSet);
}

static LRESULT CALLBACK child(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_CREATE) {
        CREATESTRUCTW *create = (CREATESTRUCTW *)lparam;
        const WORD *data = (const WORD *)create->lpCreateParams;
        printf("{\"operation\":\"dialog-child-create\",\"extended\":%u,\"modal\":%u,\"data\":[%u,%u,%u]}\n",
               extended_mode, modal_mode, data ? data[0] : 0,
               data ? data[1] : 0, data ? data[2] : 0);
    } else if (message == WM_SETFONT) {
        child_font = (HFONT)wparam;
        metrics(child_font);
        return 0;
    } else if (message == WM_GETFONT) {
        return (LRESULT)child_font;
    } else if (message == WM_DESTROY || message == WM_NCDESTROY) {
        printf("{\"operation\":\"dialog-child-destroy\",\"extended\":%u,\"modal\":%u,\"message\":%u}\n",
               extended_mode, modal_mode, message);
    }
    return DefWindowProcW(hwnd, message, wparam, lparam);
}

static INT_PTR CALLBACK dialog(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    (void)wparam;
    if (message == WM_INITDIALOG) {
        HWND control = GetDlgItem(hwnd, 100);
        RECT rectangle = { 0, 0, 4, 8 };
        BOOL mapped = MapDialogRect(hwnd, &rectangle);
        printf("{\"operation\":\"dialog-init\",\"extended\":%u,\"modal\":%u,\"parameter\":%lu,\"child\":%d,\"childId\":%d,\"mapped\":%d,\"baseX\":%ld,\"baseY\":%ld}\n",
               extended_mode, modal_mode, (unsigned long)lparam, control != NULL,
               GetDlgCtrlID(control), !!mapped, (long)rectangle.right, (long)rectangle.bottom);
        if (modal_mode) {
            BOOL ended = EndDialog(hwnd, 0x12345678);
            printf("{\"operation\":\"dialog-end\",\"extended\":%u,\"ok\":%d}\n", extended_mode, !!ended);
        }
        return TRUE;
    }
    if (message == WM_DESTROY || message == WM_NCDESTROY) {
        printf("{\"operation\":\"dialog-destroy\",\"extended\":%u,\"modal\":%u,\"message\":%u}\n",
               extended_mode, modal_mode, message);
    }
    return FALSE;
}

static void make(BYTE *bytes, BOOL extended) {
    BYTE *p = bytes;
    memset(bytes, 0, 1024);
    if (extended) {
        p = word(p, 1); p = word(p, 0xffff);
        p = dword(p, 0x11223344); p = dword(p, 0);
        p = dword(p, WS_POPUP | DS_SETFONT);
    } else {
        p = dword(p, WS_POPUP | DS_SETFONT); p = dword(p, 0);
    }
    p = word(p, 1); p = word(p, 0); p = word(p, 0);
    p = word(p, 120); p = word(p, 80);
    p = word(p, 0); p = word(p, 0); p = text(p, L"dialog probe");
    p = word(p, 9);
    if (extended) { p = word(p, 700); p = word(p, 0x0101); }
    p = text(p, L"Arial");
    p = align4(p);
    if (extended) { p = dword(p, 0x55667788); p = dword(p, 0); p = dword(p, WS_CHILD | WS_VISIBLE); }
    else { p = dword(p, WS_CHILD | WS_VISIBLE); p = dword(p, 0); }
    p = word(p, 4); p = word(p, 8); p = word(p, 20); p = word(p, 12);
    if (extended) p = dword(p, 100); else p = word(p, 100);
    p = text(p, L"Ring3DialogProbeChild"); p = text(p, L"child");
    p = word(p, extended ? 4 : 6);
    p = word(p, 0x1234); p = word(p, 0x5678);
    (void)p;
}

int main(void) {
    WNDCLASSW cls;
    memset(&cls, 0, sizeof(cls));
    cls.hInstance = GetModuleHandleW(NULL);
    cls.lpfnWndProc = child;
    cls.lpszClassName = L"Ring3DialogProbeChild";
    if (!RegisterClassW(&cls)) return 1;
    union { DWORD alignment; BYTE bytes[1024]; } input;
    for (extended_mode = 0; extended_mode != 2; ++extended_mode) {
        for (modal_mode = 0; modal_mode != 2; ++modal_mode) {
            child_font = NULL;
            make(input.bytes, extended_mode != 0);
            SetLastError(0);
            if (modal_mode) {
                INT_PTR result = DialogBoxIndirectParamW(cls.hInstance, (const DLGTEMPLATE *)input.bytes,
                                                         NULL, dialog, 0xaabbccdd);
                DWORD error = GetLastError();
                printf("{\"operation\":\"dialog-modal\",\"extended\":%u,\"result\":%ld,\"error\":%lu}\n",
                       extended_mode, (long)result, (unsigned long)error);
                if (result != 0x12345678) return 2;
            } else {
                HWND hwnd = CreateDialogIndirectParamW(cls.hInstance, (const DLGTEMPLATE *)input.bytes,
                                                       NULL, dialog, 0xaabbccdd);
                DWORD error = GetLastError();
                printf("{\"operation\":\"dialog-modeless\",\"extended\":%u,\"ok\":%d,\"error\":%lu}\n",
                       extended_mode, hwnd != NULL, (unsigned long)error);
                if (!hwnd || !DestroyWindow(hwnd)) return 3;
            }
        }
    }
    if (!UnregisterClassW(cls.lpszClassName, cls.hInstance)) return 4;
    return 0;
}
