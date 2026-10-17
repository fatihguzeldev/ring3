#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static unsigned phase;
static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_NCCALCSIZE) {
        RECT before = *(RECT *)lparam;
        LRESULT result = DefWindowProcW(window, message, wparam, lparam);
        RECT after = *(RECT *)lparam;
        printf("{\"operation\":\"menubar-nccalc\",\"phase\":%u,\"wparam\":%u,\"before\":[%ld,%ld,%ld,%ld],\"after\":[%ld,%ld,%ld,%ld],\"result\":%ld}\n",
               phase, (unsigned)wparam, before.left, before.top, before.right, before.bottom,
               after.left, after.top, after.right, after.bottom, (long)result);
        return result;
    }
    if (message == WM_WINDOWPOSCHANGING || message == WM_WINDOWPOSCHANGED) {
        WINDOWPOS *position = (WINDOWPOS *)lparam;
        printf("{\"operation\":\"menubar-position\",\"phase\":%u,\"message\":%u,\"flags\":%u}\n", phase, message, position->flags);
    }
    if (message == WM_NCPAINT || message == WM_SIZE || message == WM_MOVE)
        printf("{\"operation\":\"menubar-message\",\"phase\":%u,\"message\":%u}\n", phase, message);
    if (message == WM_MEASUREITEM) {
        MEASUREITEMSTRUCT *item = (MEASUREITEMSTRUCT *)lparam;
        printf("{\"operation\":\"menubar-measure\",\"phase\":%u,\"id\":%u,\"height\":%u}\n", phase, item->itemID, item->itemHeight);
        item->itemWidth = 53; item->itemHeight = 17; return TRUE;
    }
    if (message == WM_DRAWITEM) {
        DRAWITEMSTRUCT *item = (DRAWITEMSTRUCT *)lparam;
        printf("{\"operation\":\"menubar-draw\",\"phase\":%u,\"id\":%u,\"action\":%u,\"rect\":[%ld,%ld,%ld,%ld]}\n",
               phase, item->itemID, item->itemAction, item->rcItem.left, item->rcItem.top, item->rcItem.right, item->rcItem.bottom);
        return TRUE;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}
static void geometry(HWND window, HMENU menu, const char *name, BOOL ok) {
    RECT client = {0}, outer = {0}, item = {0}; POINT origin = {0};
    GetClientRect(window, &client); GetWindowRect(window, &outer); ClientToScreen(window, &origin);
    BOOL located = menu ? GetMenuItemRect(window, menu, 0, &item) : FALSE;
    printf("{\"operation\":\"menubar-geometry\",\"phase\":%u,\"name\":\"%s\",\"ok\":%d,\"client\":[%ld,%ld,%ld,%ld],\"outer\":[%ld,%ld,%ld,%ld],\"origin\":[%ld,%ld],\"located\":%d,\"item\":[%ld,%ld,%ld,%ld]}\n",
           phase, name, ok, client.left, client.top, client.right, client.bottom,
           outer.left, outer.top, outer.right, outer.bottom, origin.x, origin.y,
           located, item.left, item.top, item.right, item.bottom);
}
int main(void) {
    WNDCLASSW type; memset(&type, 0, sizeof(type));
    type.hInstance = GetModuleHandleW(NULL); type.lpfnWndProc = procedure;
    type.lpszClassName = L"ring3-menubar-layout";
    if (!RegisterClassW(&type)) return 1;
    HWND window = CreateWindowW(type.lpszClassName, L"bar", WS_OVERLAPPEDWINDOW,
                                20, 30, 240, 180, NULL, NULL, type.hInstance, NULL);
    if (!window) return 2;
    ShowWindow(window, SW_SHOW);
    HMENU menu = CreateMenu(); if (!menu || !AppendMenuW(menu, MF_OWNERDRAW, 37, (LPCWSTR)(ULONG_PTR)0x13572468)) return 3;
    geometry(window, NULL, "before", TRUE);
    phase = 1; BOOL assigned = SetMenu(window, menu); geometry(window, menu, "assigned", assigned);
    phase = 2; BOOL painted = DrawMenuBar(window); geometry(window, menu, "painted", painted);
    phase = 3; BOOL detached = SetMenu(window, NULL); geometry(window, NULL, "detached", detached);
    printf("{\"operation\":\"menubar-detached-live\",\"live\":%d}\n", IsMenu(menu));
    DestroyMenu(menu); DestroyWindow(window); return 0;
}
