#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static unsigned height, scenario, draws;
static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_MEASUREITEM) {
        MEASUREITEMSTRUCT *item = (MEASUREITEMSTRUCT *)lparam;
        item->itemWidth = 53; item->itemHeight = height; return TRUE;
    }
    if (message == WM_DRAWITEM) {
        DRAWITEMSTRUCT *item = (DRAWITEMSTRUCT *)lparam;
        ++draws;
        printf("{\"operation\":\"menubar-size-draw\",\"scenario\":%u,\"action\":%u,\"state\":%u,\"width\":%ld,\"height\":%ld}\n",
               scenario, item->itemAction, item->itemState,
               item->rcItem.right - item->rcItem.left, item->rcItem.bottom - item->rcItem.top);
        return TRUE;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

int main(void) {
    WNDCLASSW type; memset(&type, 0, sizeof(type));
    type.hInstance = GetModuleHandleW(NULL); type.lpfnWndProc = procedure;
    type.lpszClassName = L"ring3-menubar-sizes";
    if (!RegisterClassW(&type)) return 1;
    HWND window = CreateWindowW(type.lpszClassName, L"sizes", WS_OVERLAPPEDWINDOW,
                                0, 0, 240, 180, NULL, NULL, type.hInstance, NULL);
    if (!window) return 2;
    ShowWindow(window, SW_SHOW);
    const unsigned sizes[] = {0, 17, 40};
    for (scenario = 0; scenario < sizeof(sizes) / sizeof(sizes[0]); ++scenario) {
        height = sizes[scenario]; draws = 0;
        HMENU menu = CreateMenu();
        if (!menu || !AppendMenuW(menu, MF_OWNERDRAW, 37, (LPCWSTR)(ULONG_PTR)0x1234)) return 3;
        BOOL assigned = SetMenu(window, menu); RECT item = {0};
        BOOL located = GetMenuItemRect(window, menu, 0, &item);
        printf("{\"operation\":\"menubar-size\",\"scenario\":%u,\"requestedHeight\":%u,\"assigned\":%d,\"located\":%d,\"height\":%ld,\"draws\":%u}\n",
               scenario, height, assigned, located, item.bottom - item.top, draws);
        if (!SetMenu(window, NULL)) return 4;
        DestroyMenu(menu);
    }
    DestroyWindow(window); return 0;
}
