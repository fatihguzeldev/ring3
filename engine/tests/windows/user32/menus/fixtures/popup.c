#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static unsigned scenario, phase, ticks, cancel_menu;
static HWND owner;
static HMENU active_menu;

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_ENTERMENULOOP || message == WM_EXITMENULOOP || message == WM_INITMENU ||
        message == WM_INITMENUPOPUP || message == WM_MENUSELECT || message == WM_COMMAND ||
        message == WM_UNINITMENUPOPUP) {
        printf("{\"operation\":\"popup-callback\",\"scenario\":%u,\"phase\":%u,\"message\":%u,\"low\":%u,\"high\":%u,\"lparamZero\":%d,\"menuEqual\":%d}\n",
               scenario, phase, message, LOWORD(wparam), HIWORD(wparam), lparam == 0,
               (HMENU)wparam == active_menu);
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

static void CALLBACK input_timer(HWND window, UINT message, UINT_PTR id, DWORD now) {
    (void)message; (void)now;
    ++ticks;
    if (ticks == 1) PostMessageW(window, WM_KEYDOWN, cancel_menu ? VK_ESCAPE : VK_DOWN, 0);
    if (ticks == 2 && !cancel_menu) PostMessageW(window, WM_KEYDOWN, VK_RETURN, 0);
    if (ticks >= 5) { EndMenu(); KillTimer(window, id); }
}

struct Escape { DWORD thread; HWND window; HANDLE finished; };
static DWORD WINAPI bounded_escape(void *parameter) {
    struct Escape *escape = parameter;
    if (WaitForSingleObject(escape->finished, 1500) == WAIT_TIMEOUT) {
        PostMessageW(escape->window, WM_CANCELMODE, 0, 0);
        PostThreadMessageW(escape->thread, WM_QUIT, 0, 0);
    }
    return 0;
}

static void join_escape(HANDLE worker) {
    if (WaitForSingleObject(worker, 2000) != WAIT_OBJECT_0) {
        printf("{\"operation\":\"popup-setup\",\"scenario\":%u,\"ok\":0,\"joinFailed\":1}\n", scenario);
        fflush(stdout);
        ExitProcess(3);
    }
    CloseHandle(worker);
}

static void run(unsigned flags, BOOL cancel) {
    MSG message;
    while (PeekMessageW(&message, NULL, WM_QUIT, WM_QUIT, PM_REMOVE)) { }
    active_menu = CreatePopupMenu();
    AppendMenuW(active_menu, MF_STRING, 37, L"one");
    AppendMenuW(active_menu, MF_STRING, 38, L"two");
    ticks = 0; cancel_menu = !!cancel;
    HANDLE finished = CreateEventW(NULL, TRUE, FALSE, NULL);
    struct Escape escape = { GetCurrentThreadId(), owner, finished };
    HANDLE timeout = finished ? CreateThread(NULL, 0, bounded_escape, &escape, 0, NULL) : NULL;
    UINT_PTR timer = SetTimer(owner, 1, 20, input_timer);
    if (!finished || !timeout || !timer) {
        printf("{\"operation\":\"popup-setup\",\"scenario\":%u,\"ok\":0}\n", scenario);
        if (timer) KillTimer(owner, timer);
        if (finished) SetEvent(finished);
        if (timeout) join_escape(timeout);
        if (finished) CloseHandle(finished);
        DestroyMenu(active_menu);
        return;
    }
    phase = 1;
    SetLastError(0);
    UINT result = TrackPopupMenu(active_menu, flags | TPM_NOANIMATION, 40, 40, 0, owner, NULL);
    DWORD error = GetLastError();
    phase = 2;
    SetEvent(finished);
    KillTimer(owner, timer);
    while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
        if (message.message != WM_QUIT) DispatchMessageW(&message);
    }
    printf("{\"operation\":\"popup-result\",\"scenario\":%u,\"flags\":%u,\"cancel\":%d,\"result\":%u,\"error\":%lu,\"captureCleared\":%d,\"menuLive\":%d}\n",
           scenario, flags, !!cancel, result, (unsigned long)error, GetCapture() == NULL, !!IsMenu(active_menu));
    join_escape(timeout);
    CloseHandle(finished);
    DestroyMenu(active_menu);
}

int main(void) {
    WNDCLASSW type;
    memset(&type, 0, sizeof(type));
    type.lpfnWndProc = procedure;
    type.hInstance = GetModuleHandleW(NULL);
    type.lpszClassName = L"ring3-popup-reference";
    if (!RegisterClassW(&type)) return 1;
    owner = CreateWindowW(type.lpszClassName, L"popup", WS_OVERLAPPEDWINDOW,
                          0, 0, 200, 140, NULL, NULL, type.hInstance, NULL);
    if (!owner) return 2;
    ShowWindow(owner, SW_SHOW);
    SetForegroundWindow(owner);
    unsigned flags[] = {0, TPM_RETURNCMD, TPM_NONOTIFY, TPM_RETURNCMD | TPM_NONOTIFY};
    for (unsigned i = 0; i != sizeof(flags) / sizeof(flags[0]); ++i) {
        scenario = i; run(flags[i], FALSE);
    }
    scenario = 4; run(0, TRUE);
    scenario = 5; run(TPM_RETURNCMD, TRUE);
    DestroyWindow(owner);
    return 0;
}
