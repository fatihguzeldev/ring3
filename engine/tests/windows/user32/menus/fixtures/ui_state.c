#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static unsigned scenario, callbacks;
static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_CHANGEUISTATE || message == WM_UPDATEUISTATE) {
        ++callbacks;
        printf("{\"operation\":\"menu-ui-state-callback\",\"scenario\":%u,\"message\":%u,\"action\":%u,\"flags\":%u,\"child\":%d}\n",
               scenario, message, LOWORD(wparam), HIWORD(wparam), GetParent(window) != NULL);
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

static void state(const char *name, HWND root, HWND child) {
    printf("{\"operation\":\"menu-ui-state\",\"scenario\":%u,\"name\":\"%s\",\"root\":%lu,\"child\":%lu,\"callbacks\":%u}\n",
           scenario, name, (unsigned long)SendMessageW(root, WM_QUERYUISTATE, 0, 0),
           (unsigned long)SendMessageW(child, WM_QUERYUISTATE, 0, 0), callbacks);
}

int main(void) {
    WNDCLASSW type;
    memset(&type, 0, sizeof(type)); type.lpfnWndProc = procedure;
    type.hInstance = GetModuleHandleW(NULL); type.lpszClassName = L"ring3-menu-ui-state";
    if (!RegisterClassW(&type)) return 1;
    printf("{\"operation\":\"menu-metrics\",\"menuHeight\":%d,\"checkWidth\":%d,\"checkHeight\":%d,\"borderX\":%d,\"borderY\":%d}\n",
           GetSystemMetrics(SM_CYMENU), GetSystemMetrics(SM_CXMENUCHECK), GetSystemMetrics(SM_CYMENUCHECK),
           GetSystemMetrics(SM_CXBORDER), GetSystemMetrics(SM_CYBORDER));
    HWND root = CreateWindowW(type.lpszClassName, L"state", WS_OVERLAPPEDWINDOW,
                              0, 0, 200, 160, NULL, NULL, type.hInstance, NULL);
    HWND child = root ? CreateWindowW(type.lpszClassName, L"child", WS_CHILD,
                                     0, 0, 80, 30, root, (HMENU)(ULONG_PTR)1, type.hInstance, NULL) : NULL;
    if (!root || !child) return 2;
    state("created", root, child);
    ShowWindow(root, SW_SHOW); state("shown", root, child);
    scenario = 1;
    SendMessageW(root, WM_CHANGEUISTATE, MAKEWPARAM(UIS_SET, UISF_HIDEFOCUS | UISF_HIDEACCEL), 0);
    state("set-both", root, child);
    scenario = 2;
    SendMessageW(root, WM_CHANGEUISTATE, MAKEWPARAM(UIS_CLEAR, UISF_HIDEACCEL), 0);
    state("clear-accel", root, child);
    scenario = 3;
    SendMessageW(child, WM_CHANGEUISTATE, MAKEWPARAM(UIS_SET, UISF_HIDEACCEL), 0);
    state("child-set-accel", root, child);
    scenario = 4;
    SendMessageW(root, WM_UPDATEUISTATE, MAKEWPARAM(UIS_CLEAR, UISF_HIDEFOCUS), 0);
    state("update-clear-focus", root, child);
    scenario = 5;
    SendMessageW(root, WM_UPDATEUISTATE, MAKEWPARAM(0x55, 7), 0);
    state("invalid-action", root, child);
    scenario = 6;
    SendMessageW(root, WM_CHANGEUISTATE, MAKEWPARAM(UIS_INITIALIZE, 7), 0);
    state("initialize", root, child);
    DestroyWindow(root);
    return 0;
}
