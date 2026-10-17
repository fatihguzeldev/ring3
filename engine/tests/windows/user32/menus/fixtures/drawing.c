#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static HWND owner;
static HMENU active_menu;
static unsigned scenario, phase, zero_extent, ticks, measures, draws;
static HANDLE finished;
static DWORD owner_thread;

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_MEASUREITEM) {
        MEASUREITEMSTRUCT *item = (MEASUREITEMSTRUCT *)lparam;
        ++measures;
        printf("{\"operation\":\"menu-measure\",\"scenario\":%u,\"phase\":%u,\"wparam\":%u,\"type\":%u,\"control\":%u,\"id\":%u,\"initialWidth\":%u,\"initialHeight\":%u,\"data\":%lu}\n",
               scenario, phase, (unsigned)wparam, item->CtlType, item->CtlID, item->itemID,
               item->itemWidth, item->itemHeight, (unsigned long)item->itemData);
        item->itemWidth = zero_extent ? 0 : 53;
        item->itemHeight = zero_extent ? 0 : 17;
        return TRUE;
    }
    if (message == WM_DRAWITEM) {
        DRAWITEMSTRUCT *item = (DRAWITEMSTRUCT *)lparam;
        ++draws;
        COLORREF before = GetPixel(item->hDC, item->rcItem.left, item->rcItem.top);
        HBRUSH brush = CreateSolidBrush(RGB(0x22, 0x44, 0x66));
        BOOL fill = brush ? FillRect(item->hDC, &item->rcItem, brush) : FALSE;
        COLORREF after = GetPixel(item->hDC, item->rcItem.left, item->rcItem.top);
        if (brush) DeleteObject(brush);
        printf("{\"operation\":\"menu-draw\",\"scenario\":%u,\"phase\":%u,\"wparam\":%u,\"type\":%u,\"control\":%u,\"id\":%u,\"action\":%u,\"state\":%u,\"menuEqual\":%d,\"dcNonzero\":%d,\"left\":%ld,\"top\":%ld,\"right\":%ld,\"bottom\":%ld,\"data\":%lu,\"fill\":%d,\"before\":%lu,\"after\":%lu}\n",
               scenario, phase, (unsigned)wparam, item->CtlType, item->CtlID, item->itemID,
               item->itemAction, item->itemState, (HMENU)item->hwndItem == active_menu,
               item->hDC != NULL, item->rcItem.left, item->rcItem.top,
               item->rcItem.right, item->rcItem.bottom, (unsigned long)item->itemData,
               fill, (unsigned long)before, (unsigned long)after);
        return TRUE;
    }
    if (message == WM_INITMENU || message == WM_INITMENUPOPUP || message == WM_MENUSELECT ||
        message == WM_UNINITMENUPOPUP || message == WM_ENTERMENULOOP || message == WM_EXITMENULOOP ||
        message == WM_COMMAND)
        printf("{\"operation\":\"menu-drawing-callback\",\"scenario\":%u,\"phase\":%u,\"message\":%u,\"low\":%u,\"high\":%u}\n",
               scenario, phase, message, LOWORD(wparam), HIWORD(wparam));
    return DefWindowProcW(window, message, wparam, lparam);
}

static void CALLBACK input_timer(HWND window, UINT message, UINT_PTR id, DWORD now) {
    (void)message; (void)now;
    ++ticks;
    if (ticks == 1) PostMessageW(window, WM_KEYDOWN, VK_DOWN, 0);
    if (ticks == 2) PostMessageW(window, WM_KEYDOWN, VK_RETURN, 0);
    if (ticks == 10) { EndMenu(); KillTimer(window, id); }
}

static DWORD WINAPI escape_worker(void *unused) {
    (void)unused;
    if (WaitForSingleObject(finished, 1500) == WAIT_TIMEOUT) {
        PostMessageW(owner, WM_CANCELMODE, 0, 0);
        PostThreadMessageW(owner_thread, WM_QUIT, 0, 0);
    }
    return 0;
}

static void join(HANDLE worker) {
    if (WaitForSingleObject(worker, 2000) != WAIT_OBJECT_0) {
        puts("{\"operation\":\"menu-drawing-setup\",\"ok\":0,\"joinFailed\":1}");
        fflush(stdout);
        ExitProcess(3);
    }
    CloseHandle(worker);
}

static void popup(unsigned number, BOOL zero) {
    MSG message;
    scenario = number; phase = ticks = measures = draws = 0; zero_extent = zero;
    active_menu = CreatePopupMenu();
    if (!active_menu || !AppendMenuW(active_menu, MF_OWNERDRAW, 37, (LPCWSTR)(ULONG_PTR)0x13572468)) ExitProcess(4);
    finished = CreateEventW(NULL, TRUE, FALSE, NULL);
    HANDLE worker = finished ? CreateThread(NULL, 0, escape_worker, NULL, 0, NULL) : NULL;
    UINT_PTR timer = worker ? SetTimer(owner, 1, 20, input_timer) : 0;
    if (!worker || !timer) {
        if (finished) SetEvent(finished);
        if (worker) join(worker);
        ExitProcess(5);
    }
    phase = 1;
    SetLastError(0);
    UINT result = TrackPopupMenu(active_menu, TPM_NOANIMATION | TPM_RETURNCMD, 40, 40, 0, owner, NULL);
    DWORD error = GetLastError();
    phase = 2;
    SetEvent(finished); KillTimer(owner, timer); join(worker); CloseHandle(finished);
    while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) if (message.message != WM_QUIT) DispatchMessageW(&message);
    printf("{\"operation\":\"menu-drawing-result\",\"scenario\":%u,\"result\":%u,\"error\":%lu,\"measures\":%u,\"draws\":%u}\n",
           scenario, result, (unsigned long)error, measures, draws);
    DestroyMenu(active_menu);
}

int main(void) {
    WNDCLASSW type;
    memset(&type, 0, sizeof(type));
    type.lpfnWndProc = procedure; type.hInstance = GetModuleHandleW(NULL);
    type.lpszClassName = L"ring3-menu-drawing-reference";
    if (!RegisterClassW(&type)) return 1;
    owner = CreateWindowW(type.lpszClassName, L"menu drawing", WS_OVERLAPPEDWINDOW,
                         0, 0, 240, 180, NULL, NULL, type.hInstance, NULL);
    if (!owner) return 2;
    owner_thread = GetCurrentThreadId(); ShowWindow(owner, SW_SHOW); SetForegroundWindow(owner);
    popup(0, FALSE); popup(1, TRUE);
    scenario = 2; phase = measures = draws = zero_extent = 0;
    active_menu = CreateMenu();
    if (!active_menu || !AppendMenuW(active_menu, MF_OWNERDRAW, 37, (LPCWSTR)(ULONG_PTR)0x13572468)) return 6;
    BOOL attached = SetMenu(owner, active_menu);
    phase = 1;
    BOOL painted = DrawMenuBar(owner);
    RECT item = {0}; BOOL located = GetMenuItemRect(owner, active_menu, 0, &item);
    printf("{\"operation\":\"menubar-drawing-result\",\"attached\":%d,\"painted\":%d,\"located\":%d,\"width\":%ld,\"height\":%ld,\"measures\":%u,\"draws\":%u}\n",
           attached, painted, located, item.right - item.left, item.bottom - item.top, measures, draws);
    DestroyWindow(owner);
    return 0;
}
