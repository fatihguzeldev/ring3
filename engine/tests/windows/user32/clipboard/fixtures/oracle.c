#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static UINT delayed_format;
static unsigned rendered, destroyed;

static HGLOBAL payload(void) {
    HGLOBAL data = GlobalAlloc(GMEM_MOVEABLE, 8);
    if (!data) return NULL;
    WCHAR *text = (WCHAR *)GlobalLock(data);
    if (!text) { GlobalFree(data); return NULL; }
    text[0] = L'A'; text[1] = 0x00df; text[2] = 0; text[3] = 0xeeee;
    GlobalUnlock(data);
    return data;
}

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_RENDERFORMAT && wparam == delayed_format) {
        ++rendered;
        HGLOBAL data = payload();
        if (data && !SetClipboardData(delayed_format, data)) GlobalFree(data);
        return 0;
    }
    if (message == WM_DESTROYCLIPBOARD) { ++destroyed; return 0; }
    return DefWindowProcW(window, message, wparam, lparam);
}

static void formats(const char *name) {
    printf("{\"operation\":\"clipboard-formats\",\"case\":\"%s\",\"count\":%d,\"text\":%d,\"unicode\":%d,\"oem\":%d,\"entries\":[",
           name, CountClipboardFormats(), !!IsClipboardFormatAvailable(CF_TEXT),
           !!IsClipboardFormatAvailable(CF_UNICODETEXT), !!IsClipboardFormatAvailable(CF_OEMTEXT));
    UINT previous = 0;
    for (unsigned i = 0; i < 32; ++i) {
        UINT next = EnumClipboardFormats(previous);
        if (!next) break;
        printf("%s%u", i ? "," : "", next);
        previous = next;
    }
    puts("]}");
}

static void placed(HWND window, const char *name) {
    SetLastError(0xabcdef01);
    BOOL opened = OpenClipboard(window);
    DWORD open_error = GetLastError();
    if (!opened) {
        printf("{\"operation\":\"clipboard-place\",\"case\":\"%s\",\"open\":0,\"error\":%lu}\n", name, open_error);
        return;
    }
    DWORD sequence = GetClipboardSequenceNumber();
    BOOL emptied = EmptyClipboard();
    HGLOBAL data = payload();
    SetLastError(0xabcdef01);
    HANDLE stored = SetClipboardData(CF_UNICODETEXT, data);
    DWORD error = GetLastError();
    printf("{\"operation\":\"clipboard-place\",\"case\":\"%s\",\"open\":1,\"openError\":%lu,\"empty\":%d,\"set\":%d,\"error\":%lu,\"ownerEqual\":%d,\"openWindowEqual\":%d,\"sequenceDelta\":%lu,\"size\":%lu}\n",
           name, open_error, !!emptied, stored != NULL, error,
           GetClipboardOwner() == window, GetOpenClipboardWindow() == window,
           GetClipboardSequenceNumber() - sequence, (DWORD)GlobalSize(data));
    if (!stored && data) GlobalFree(data);
    formats("before-close");
    CloseClipboard();
    if (OpenClipboard(window)) {
        formats("after-close");
        SetLastError(0xabcdef01);
        HGLOBAL text = GetClipboardData(CF_TEXT);
        DWORD text_error = GetLastError();
        BYTE first[4] = {0xee,0xee,0xee,0xee};
        if (text) {
            BYTE *bytes = GlobalLock(text);
            if (bytes) { memcpy(first, bytes, min(GlobalSize(text), sizeof(first))); GlobalUnlock(text); }
        }
        printf("{\"operation\":\"clipboard-text\",\"available\":%d,\"error\":%lu,\"bytes\":[%u,%u,%u,%u]}\n",
               text != NULL, text_error, first[0], first[1], first[2], first[3]);
        EmptyClipboard();
        CloseClipboard();
    }
}

static void query_row(const char *name, ULONG_PTR result, DWORD error) {
    printf("{\"operation\":\"clipboard-query\",\"case\":\"%s\",\"result\":%llu,\"error\":%lu}\n", name, (unsigned long long)result, error);
}
static void query_reference(HWND window) {
    WCHAR name[4] = {0xeeee,0xeeee,0xeeee,0xeeee};
    UINT formats[] = {CF_UNICODETEXT, CF_TEXT};
    SetLastError(0xabcdef01); UINT result = EnumClipboardFormats(0); DWORD error = GetLastError();
    query_row("enum-closed", result, error);
    SetLastError(0xabcdef01); HANDLE data = GetClipboardData(CF_UNICODETEXT); error = GetLastError();
    query_row("get-closed", (ULONG_PTR)data, error);
    SetLastError(0xabcdef01); result = EmptyClipboard(); error = GetLastError();
    query_row("empty-closed", result, error);
    SetLastError(0xabcdef01); data = SetClipboardData(CF_UNICODETEXT, NULL); error = GetLastError();
    query_row("set-closed", (ULONG_PTR)data, error);
    SetLastError(0xabcdef01); result = GetClipboardFormatNameW(CF_UNICODETEXT, name, 4); error = GetLastError();
    query_row("name-predefined", result, error);
    SetLastError(0xabcdef01); result = GetClipboardFormatNameW(delayed_format, name, 0); error = GetLastError();
    query_row("name-zero-capacity", result, error);
    if (!OpenClipboard(window)) ExitProcess(2);
    EmptyClipboard();
    SetLastError(0xabcdef01); result = EnumClipboardFormats(0); error = GetLastError();
    query_row("enum-empty", result, error);
    SetLastError(0xabcdef01); result = EnumClipboardFormats(CF_TEXT); error = GetLastError();
    query_row("enum-invalid-previous", result, error);
    SetLastError(0xabcdef01); result = GetPriorityClipboardFormat(formats, 2); error = GetLastError();
    query_row("priority-empty", result, error);
    SetLastError(0xabcdef01); data = SetClipboardData(0, NULL); error = GetLastError();
    query_row("set-format-zero", (ULONG_PTR)data, error);
    SetClipboardData(CF_UNICODETEXT, NULL);
    CloseClipboard();
    if (!OpenClipboard(window)) ExitProcess(2);
    SetLastError(0xabcdef01); data = GetClipboardData(CF_LOCALE); error = GetLastError();
    DWORD locale = 0;
    if (data) { DWORD *value = GlobalLock(data); if (value && GlobalSize(data) >= 4) locale = *value; if (value) GlobalUnlock(data); }
    printf("{\"operation\":\"clipboard-query\",\"case\":\"locale-from-delayed-text\",\"result\":%u,\"error\":%lu,\"locale\":%lu,\"inputLocale\":%u}\n", data != NULL, error, locale, LOWORD(GetKeyboardLayout(0)));
    EmptyClipboard(); CloseClipboard();
}

static void retired_global_reference(void) {
    HGLOBAL block = GlobalAlloc(GMEM_MOVEABLE, 8);
    if (!block || GlobalFree(block)) ExitProcess(2);
    SetLastError(0xabcdef01);
    SIZE_T size = GlobalSize(block);
    DWORD error = GetLastError();
    query_row("size-freed-movable", size, error);
}

int main(void) {
    WNDCLASSW type;
    memset(&type, 0, sizeof(type));
    type.lpfnWndProc = procedure; type.hInstance = GetModuleHandleW(NULL);
    type.lpszClassName = L"ring3-clipboard-reference";
    if (!RegisterClassW(&type)) return 1;
    HWND window = CreateWindowW(type.lpszClassName, L"clipboard", WS_POPUP,
                                0, 0, 1, 1, NULL, NULL, type.hInstance, NULL);
    if (!window) return 1;
    placed(NULL, "null-owner"); placed(window, "window-owner");
    if (!OpenClipboard(window)) { DestroyWindow(window); return 1; }
    SetLastError(0xabcdef01);
    BOOL reopened = OpenClipboard(window);
    DWORD reopen_error = GetLastError();
    EmptyClipboard();
    delayed_format = RegisterClipboardFormatW(L"Ring3 delayed data");
    UINT lower = RegisterClipboardFormatW(L"ring3 DELAYED DATA");
    WCHAR name[4] = {0xeeee,0xeeee,0xeeee,0xeeee};
    int copied = GetClipboardFormatNameW(delayed_format, name, 4);
    SetLastError(0xabcdef01);
    HANDLE delayed = SetClipboardData(delayed_format, NULL);
    DWORD delayed_error = GetLastError();
    unsigned before = rendered;
    SetLastError(0xabcdef01);
    HANDLE value = GetClipboardData(delayed_format);
    DWORD get_error = GetLastError();
    printf("{\"operation\":\"clipboard-delayed\",\"reopen\":%d,\"reopenError\":%lu,\"caseEqual\":%d,\"nameCount\":%d,\"name\":[%u,%u,%u,%u],\"setNull\":%d,\"setError\":%lu,\"get\":%d,\"getError\":%lu,\"renders\":%u,\"destroyed\":%u}\n",
           !!reopened, reopen_error, delayed_format == lower, copied,
           name[0], name[1], name[2], name[3], delayed != NULL, delayed_error,
           value != NULL, get_error, rendered - before, destroyed);
    EmptyClipboard();
    BOOL first = CloseClipboard(); SetLastError(0xabcdef01);
    BOOL second = CloseClipboard(); DWORD close_error = GetLastError();
    printf("{\"operation\":\"clipboard-close\",\"first\":%d,\"second\":%d,\"error\":%lu}\n", !!first, !!second, close_error);
    query_reference(window);
    retired_global_reference();
    DestroyWindow(window);
    return 0;
}
