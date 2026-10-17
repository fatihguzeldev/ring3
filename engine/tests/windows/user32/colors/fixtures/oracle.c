#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static unsigned phase;
static HWND windows[3];

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    unsigned identity = (unsigned)GetWindowLongPtrW(window, GWLP_USERDATA);
    if (phase && phase < 99 && message == WM_SYSCOLORCHANGE) {
        LOGBRUSH brush;
        memset(&brush, 0, sizeof(brush));
        int bytes = GetObjectW(GetSysColorBrush(COLOR_BTNFACE), sizeof(brush), &brush);
        printf("{\"component\":\"user32-colors\",\"operation\":\"notify\",\"phase\":%u,\"window\":%u,\"message\":%u,\"wparam\":%lu,\"lparam\":%ld,\"color\":%lu,\"brushBytes\":%d,\"brushColor\":%lu}\n",
               phase, identity, message, (unsigned long)wparam, (long)lparam,
               (unsigned long)GetSysColor(COLOR_BTNFACE), bytes, (unsigned long)brush.lbColor);
    }
    if (message == WM_PAINT) {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        if (!dc) ExitProcess(3);
        FillRect(dc, &paint.rcPaint, GetSysColorBrush(COLOR_BTNFACE));
        COLORREF pixel = GetPixel(dc, 2, 2);
        if (phase && phase < 99) {
            printf("{\"component\":\"user32-colors\",\"operation\":\"paint\",\"phase\":%u,\"window\":%u,\"erase\":%d,\"pixel\":%lu}\n",
                   phase, identity, !!paint.fErase, (unsigned long)pixel);
        }
        EndPaint(window, &paint);
        return 0;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

static void run(int count, const int *indices, const COLORREF *values) {
    for (unsigned i = 0; i < 3; ++i) ValidateRect(windows[i], NULL);
    SetLastError(0x12345678);
    BOOL result = SetSysColors(count, indices, values);
    DWORD error = GetLastError();
    printf("{\"component\":\"user32-colors\",\"operation\":\"result\",\"phase\":%u,\"count\":%d,\"result\":%d,\"error\":%lu,\"color\":%lu}\n",
           phase, count, !!result, (unsigned long)error, (unsigned long)GetSysColor(COLOR_BTNFACE));
    for (unsigned i = 0; i < 3; ++i) {
        RECT update;
        memset(&update, 0, sizeof(update));
        BOOL dirty = GetUpdateRect(windows[i], &update, FALSE);
        printf("{\"component\":\"user32-colors\",\"operation\":\"update\",\"phase\":%u,\"window\":%u,\"dirty\":%d,\"rectangle\":[%ld,%ld,%ld,%ld]}\n",
               phase, i + 1, !!dirty, update.left, update.top, update.right, update.bottom);
    }
}

int main(void) {
    int restore_indices[31];
    COLORREF restore_values[31];
    int restore_count = 0;
    for (int i = 0; i <= 30; ++i) {
        if (i == 25) continue;
        restore_indices[restore_count] = i;
        restore_values[restore_count++] = GetSysColor(i);
    }
    WNDCLASSW type;
    memset(&type, 0, sizeof(type));
    type.lpfnWndProc = procedure;
    type.hInstance = GetModuleHandleW(NULL);
    type.lpszClassName = L"ring3-system-colors-reference";
    type.hbrBackground = GetSysColorBrush(COLOR_BTNFACE);
    if (!RegisterClassW(&type)) return 1;
    windows[0] = CreateWindowW(type.lpszClassName, L"visible", WS_OVERLAPPEDWINDOW,
                               0, 0, 180, 120, NULL, NULL, type.hInstance, NULL);
    windows[1] = windows[0] ? CreateWindowW(type.lpszClassName, L"child", WS_CHILD | WS_VISIBLE,
                               10, 10, 20, 20, windows[0], NULL, type.hInstance, NULL) : NULL;
    windows[2] = CreateWindowW(type.lpszClassName, L"hidden", WS_POPUP,
                               200, 0, 40, 40, NULL, NULL, type.hInstance, NULL);
    if (!windows[0] || !windows[1] || !windows[2]) return 2;
    for (unsigned i = 0; i < 3; ++i) SetWindowLongPtrW(windows[i], GWLP_USERDATA, i + 1);
    ShowWindow(windows[0], SW_SHOW);
    UpdateWindow(windows[0]);
    UpdateWindow(windows[1]);
    const int index = COLOR_BTNFACE;
    const COLORREF value = RGB(17, 34, 51);
    phase = 1; run(1, &index, &value);
    phase = 2; run(0, &index, &value);
    const int duplicate[2] = { COLOR_BTNFACE, COLOR_BTNFACE };
    const COLORREF duplicate_values[2] = { RGB(1, 2, 3), RGB(4, 5, 6) };
    phase = 3; run(2, duplicate, duplicate_values);
    const int reserved = 25;
    phase = 4; run(1, &reserved, &value);
    const int invalid = -1;
    phase = 5; run(1, &invalid, &value);
    phase = 99;
    BOOL restored = SetSysColors(restore_count, restore_indices, restore_values);
    for (unsigned i = 3; i != 0; --i) if (IsWindow(windows[i - 1])) DestroyWindow(windows[i - 1]);
    return restored ? 0 : 4;
}
