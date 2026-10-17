#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static HWND owner;
static HANDLE finished;
static DWORD owner_thread;
static unsigned scenario, ticks;

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_CHANGEUISTATE || message == WM_UPDATEUISTATE ||
        message == WM_ENTERMENULOOP || message == WM_INITMENU || message == WM_INITMENUPOPUP ||
        message == WM_MENUSELECT || message == WM_UNINITMENUPOPUP || message == WM_EXITMENULOOP)
        printf("{\"operation\":\"popup-ui-callback\",\"scenario\":%u,\"message\":%u,\"wparam\":%u,\"state\":%lu}\n",
               scenario, message, (unsigned)wparam,
               (unsigned long)SendMessageW(window, WM_QUERYUISTATE, 0, 0));
    if (message == WM_MEASUREITEM) {
        MEASUREITEMSTRUCT *item = (MEASUREITEMSTRUCT *)lparam;
        item->itemWidth = 53; item->itemHeight = 17; return TRUE;
    }
    if (message == WM_DRAWITEM) {
        DRAWITEMSTRUCT *item = (DRAWITEMSTRUCT *)lparam;
        printf("{\"operation\":\"popup-ui-draw\",\"scenario\":%u,\"action\":%u,\"itemState\":%u,\"state\":%lu}\n",
               scenario, item->itemAction, item->itemState,
               (unsigned long)SendMessageW(window, WM_QUERYUISTATE, 0, 0));
        return TRUE;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

static void CALLBACK input(HWND window, UINT message, UINT_PTR id, DWORD time) {
    (void)message; (void)time;
    ++ticks;
    if (ticks == 1) PostMessageW(window, WM_KEYDOWN, VK_DOWN, 0);
    if (ticks == 2) PostMessageW(window, WM_KEYDOWN, VK_RETURN, 0);
    if (ticks == 10) { EndMenu(); KillTimer(window, id); }
}

static DWORD WINAPI escape(void *unused) {
    (void)unused;
    if (WaitForSingleObject(finished, 1500) == WAIT_TIMEOUT) {
        PostMessageW(owner, WM_CANCELMODE, 0, 0);
        PostThreadMessageW(owner_thread, WM_QUIT, 0, 0);
    }
    return 0;
}

int main(void) {
    WNDCLASSW type; memset(&type, 0, sizeof(type));
    type.hInstance = GetModuleHandleW(NULL); type.lpfnWndProc = procedure;
    type.lpszClassName = L"ring3-popup-ui";
    if (!RegisterClassW(&type)) return 1;
    owner = CreateWindowW(type.lpszClassName, L"state", WS_OVERLAPPEDWINDOW,
                          0, 0, 200, 160, NULL, NULL, type.hInstance, NULL);
    if (!owner) return 2;
    owner_thread = GetCurrentThreadId(); ShowWindow(owner, SW_SHOW); SetForegroundWindow(owner);
    for (scenario = 0; scenario < 2; ++scenario) {
        ticks = 0;
        SendMessageW(owner, WM_UPDATEUISTATE, MAKEWPARAM(scenario ? UIS_SET : UIS_CLEAR, 3), 0);
        HMENU menu = CreatePopupMenu();
        if (!menu || !AppendMenuW(menu, MF_OWNERDRAW, 37, (LPCWSTR)(ULONG_PTR)0x1234)) return 3;
        finished = CreateEventW(NULL, TRUE, FALSE, NULL);
        HANDLE worker = finished ? CreateThread(NULL, 0, escape, NULL, 0, NULL) : NULL;
        UINT_PTR timer = worker ? SetTimer(owner, 1, 20, input) : 0;
        if (!worker || !timer) {
            if (finished) SetEvent(finished);
            if (worker && WaitForSingleObject(worker, 2000) != WAIT_OBJECT_0) ExitProcess(4);
            return 4;
        }
        UINT result = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NOANIMATION, 40, 40, 0, owner, NULL);
        SetEvent(finished); KillTimer(owner, timer);
        if (WaitForSingleObject(worker, 2000) != WAIT_OBJECT_0) ExitProcess(5);
        CloseHandle(worker); CloseHandle(finished); DestroyMenu(menu);
        printf("{\"operation\":\"popup-ui-result\",\"scenario\":%u,\"result\":%u,\"state\":%lu}\n",
               scenario, result, (unsigned long)SendMessageW(owner, WM_QUERYUISTATE, 0, 0));
    }
    DestroyWindow(owner); return 0;
}
