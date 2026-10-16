#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static unsigned scenario, recording;

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (recording && (message == WM_COMMAND || message == WM_SYSCOMMAND ||
                      message == WM_INITMENU || message == WM_INITMENUPOPUP)) {
        printf("{\"operation\":\"accelerator-callback\",\"scenario\":%u,\"message\":%u,\"low\":%u,\"high\":%u}\n",
               scenario, message, LOWORD(wparam), HIWORD(wparam));
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

static void menu_item(HMENU menu, UINT position, const char *name) {
    MENUITEMINFOW item;
    WCHAR text[32];
    memset(&item, 0, sizeof(item));
    memset(text, 0, sizeof(text));
    item.cbSize = sizeof(item);
    item.fMask = MIIM_FTYPE | MIIM_STATE | MIIM_ID | MIIM_SUBMENU | MIIM_STRING;
    item.dwTypeData = text;
    item.cch = 32;
    SetLastError(0);
    BOOL got = GetMenuItemInfoW(menu, position, TRUE, &item);
    DWORD error = GetLastError();
    printf("{\"operation\":\"menu-item\",\"name\":\"%s\",\"ok\":%d,\"error\":%lu,\"id\":%u,\"type\":%u,\"state\":%u,\"submenu\":%d,\"childCount\":%d,\"help\":%lu,\"text0\":%u}\n",
           name, !!got, (unsigned long)error, item.wID, item.fType, item.fState,
           !!IsMenu(item.hSubMenu), item.hSubMenu ? GetMenuItemCount(item.hSubMenu) : -1,
           (unsigned long)(item.hSubMenu ? GetMenuContextHelpId(item.hSubMenu) : 0), text[0]);
}

static BYTE *put_word(BYTE *p, WORD value) { memcpy(p, &value, 2); return p + 2; }
static BYTE *put_dword(BYTE *p, DWORD value) { memcpy(p, &value, 4); return p + 4; }
static BYTE *put_text(BYTE *p, const WCHAR *text) {
    do { p = put_word(p, *text); } while (*text++);
    return p;
}
static BYTE *align4(BYTE *p) { return (BYTE *)(((uintptr_t)p + 3) & ~(uintptr_t)3); }

static void templates(void) {
    WORD standard[] = {0, 0, 0x90, 'F', 0, 0x88, 37, 'X', 0};
    SetLastError(0);
    HMENU menu = LoadMenuIndirectW(standard);
    DWORD error = GetLastError();
    printf("{\"operation\":\"menu-template\",\"extended\":0,\"ok\":%d,\"error\":%lu,\"count\":%d}\n",
           !!menu, (unsigned long)error, GetMenuItemCount(menu));
    menu_item(menu, 0, "standard-popup");
    menu_item(GetSubMenu(menu, 0), 0, "standard-command");
    DestroyMenu(menu);

    union { DWORD aligned; BYTE bytes[128]; } extended;
    memset(&extended, 0, sizeof(extended));
    BYTE *p = extended.bytes;
    p = put_word(p, 1); p = put_word(p, 4); p = put_dword(p, 0x12345678);
    p = put_dword(p, MFT_RADIOCHECK); p = put_dword(p, 3); p = put_dword(p, 0x87654321);
    p = put_word(p, 0x81); p = put_text(p, L"P"); p = align4(p); p = put_dword(p, 0x13572468);
    p = put_dword(p, MFT_OWNERDRAW); p = put_dword(p, MFS_CHECKED); p = put_dword(p, 0xcafebeef);
    p = put_word(p, 0x80); p = put_text(p, L"Z");
    (void)p;
    SetLastError(0);
    menu = LoadMenuIndirectW(extended.bytes);
    error = GetLastError();
    printf("{\"operation\":\"menu-template\",\"extended\":1,\"ok\":%d,\"error\":%lu,\"count\":%d,\"help\":%lu}\n",
           !!menu, (unsigned long)error, GetMenuItemCount(menu), (unsigned long)GetMenuContextHelpId(menu));
    menu_item(menu, 0, "extended-popup");
    menu_item(GetSubMenu(menu, 0), 0, "extended-command");
    DestroyMenu(menu);
}

static void lifetime(HWND window) {
    for (unsigned destroy = 0; destroy != 2; ++destroy) {
        HMENU parent = CreateMenu(), child = CreatePopupMenu(), leaf = CreatePopupMenu();
        AppendMenuW(child, MF_POPUP, (UINT_PTR)leaf, L"leaf");
        AppendMenuW(parent, MF_POPUP, (UINT_PTR)child, L"child");
        BOOL ok = destroy ? DeleteMenu(parent, 0, MF_BYPOSITION) : RemoveMenu(parent, 0, MF_BYPOSITION);
        printf("{\"operation\":\"menu-detach\",\"delete\":%u,\"ok\":%d,\"childLive\":%d,\"leafLive\":%d,\"parentCount\":%d}\n",
               destroy, !!ok, !!IsMenu(child), !!IsMenu(leaf), GetMenuItemCount(parent));
        DestroyMenu(parent);
        if (!destroy) DestroyMenu(child);
    }
    HMENU first = CreateMenu(), second = CreateMenu();
    BOOL assigned = SetMenu(window, first), replaced = SetMenu(window, second);
    BOOL destroyed = DestroyWindow(window);
    printf("{\"operation\":\"menu-window-lifetime\",\"assigned\":%d,\"replaced\":%d,\"windowDestroyed\":%d,\"firstLive\":%d,\"secondLive\":%d}\n",
           !!assigned, !!replaced, !!destroyed, !!IsMenu(first), !!IsMenu(second));
    if (IsMenu(first)) DestroyMenu(first);
    if (IsMenu(second)) DestroyMenu(second);
    HMENU a = CreateMenu(), b = CreateMenu(), shared = CreatePopupMenu();
    BOOL one = AppendMenuW(a, MF_POPUP, (UINT_PTR)shared, L"one");
    BOOL two = AppendMenuW(b, MF_POPUP, (UINT_PTR)shared, L"two");
    DestroyMenu(a);
    printf("{\"operation\":\"menu-shared-submenu\",\"first\":%d,\"second\":%d,\"childLive\":%d,\"secondChildEqual\":%d}\n",
           !!one, !!two, !!IsMenu(shared), GetSubMenu(b, 0) == shared);
    DestroyMenu(b);
}

static void tables(void) {
    HINSTANCE module = GetModuleHandleW(NULL);
    HACCEL one = LoadAcceleratorsW(module, MAKEINTRESOURCEW(100));
    HACCEL two = LoadAcceleratorsW(module, MAKEINTRESOURCEW(100));
    ACCEL copied[4];
    memset(copied, 0, sizeof(copied));
    int count = CopyAcceleratorTableW(one, copied, 4);
    SetLastError(0);
    BOOL first = DestroyAcceleratorTable(one);
    DWORD first_error = GetLastError();
    int remaining = CopyAcceleratorTableW(two, NULL, 0);
    SetLastError(0);
    BOOL second = DestroyAcceleratorTable(two);
    DWORD second_error = GetLastError();
    SetLastError(0);
    int after = CopyAcceleratorTableW(two, NULL, 0);
    DWORD after_error = GetLastError();
    printf("{\"operation\":\"accelerator-resource-lifetime\",\"loaded\":%d,\"same\":%d,\"count\":%d,\"first\":%d,\"firstError\":%lu,\"remaining\":%d,\"second\":%d,\"secondError\":%lu,\"after\":%d,\"afterError\":%lu,\"flags\":[%u,%u],\"keys\":[%u,%u],\"commands\":[%u,%u]}\n",
           one != NULL && two != NULL, one == two, count, !!first, (unsigned long)first_error,
           remaining, !!second, (unsigned long)second_error, after, (unsigned long)after_error,
           copied[0].fVirt, copied[1].fVirt, copied[0].key, copied[1].key, copied[0].cmd, copied[1].cmd);
    ACCEL original = {0x41, 65, 7};
    SetLastError(0);
    HACCEL unknown = CreateAcceleratorTableW(&original, 1);
    DWORD error = GetLastError();
    memset(copied, 0, sizeof(copied));
    count = CopyAcceleratorTableW(unknown, copied, 1);
    printf("{\"operation\":\"accelerator-extra-flag\",\"ok\":%d,\"error\":%lu,\"count\":%d,\"flags\":%u}\n",
           !!unknown, (unsigned long)error, count, copied[0].fVirt);
    if (unknown) DestroyAcceleratorTable(unknown);
}

static void translate(HWND window) {
    static const struct { BYTE flags; UINT message; WORD key; BYTE modifiers; } cases[] = {
        {FVIRTKEY | FCONTROL, WM_KEYDOWN, 'A', FCONTROL},
        {FVIRTKEY | FCONTROL, WM_KEYDOWN, 'A', 0},
        {FVIRTKEY, WM_KEYDOWN, 'A', FSHIFT},
        {0, WM_CHAR, 'x', 0}, {FCONTROL, WM_CHAR, 'x', 0},
        {FCONTROL, WM_CHAR, 'x', FCONTROL}, {0, WM_KEYDOWN, 'x', 0},
        {0, WM_SYSCHAR, 'x', FALT}, {FALT, WM_SYSCHAR, 'x', FALT},
    };
    BYTE saved[256];
    GetKeyboardState(saved);
    for (scenario = 0; scenario != sizeof(cases) / sizeof(cases[0]); ++scenario) {
        BYTE state[256] = {0};
        if (cases[scenario].modifiers & FSHIFT) state[VK_SHIFT] = 0x80;
        if (cases[scenario].modifiers & FCONTROL) state[VK_CONTROL] = 0x80;
        if (cases[scenario].modifiers & FALT) state[VK_MENU] = 0x80;
        SetKeyboardState(state);
        ACCEL entry = {cases[scenario].flags, cases[scenario].key, 9};
        HACCEL table = CreateAcceleratorTableW(&entry, 1);
        MSG message;
        memset(&message, 0, sizeof(message));
        message.hwnd = window; message.message = cases[scenario].message;
        message.wParam = cases[scenario].key; message.lParam = 1;
        recording = 1;
        int result = TranslateAcceleratorW(window, table, &message);
        recording = 0;
        printf("{\"operation\":\"accelerator-translate\",\"scenario\":%u,\"flags\":%u,\"modifiers\":%u,\"message\":%u,\"result\":%d}\n",
               scenario, entry.fVirt, cases[scenario].modifiers, message.message, result);
        DestroyAcceleratorTable(table);
    }
    SetKeyboardState(saved);
    HMENU menu = CreateMenu(), child = CreatePopupMenu();
    AppendMenuW(child, MF_STRING, 9, L"command");
    AppendMenuW(menu, MF_POPUP, (UINT_PTR)child, L"popup");
    SetMenu(window, menu);
    ACCEL entry = {0, 'x', 9};
    HACCEL table = CreateAcceleratorTableW(&entry, 1);
    MSG message;
    memset(&message, 0, sizeof(message));
    message.hwnd = window; message.message = WM_CHAR; message.wParam = 'x';
    for (scenario = 10; scenario != 13; ++scenario) {
        EnableWindow(window, scenario != 11);
        EnableMenuItem(child, 9, scenario == 12 ? MF_BYCOMMAND | MF_GRAYED : MF_BYCOMMAND | MF_ENABLED);
        recording = 1;
        int result = TranslateAcceleratorW(window, table, &message);
        recording = 0;
        printf("{\"operation\":\"accelerator-menu\",\"scenario\":%u,\"result\":%d}\n", scenario, result);
    }
    DestroyAcceleratorTable(table);
    SetMenu(window, NULL);
    DestroyMenu(menu);
}

int main(void) {
    WNDCLASSW cls;
    memset(&cls, 0, sizeof(cls));
    cls.hInstance = GetModuleHandleW(NULL); cls.lpfnWndProc = procedure; cls.lpszClassName = L"Ring3MenuProbe";
    if (!RegisterClassW(&cls)) return 1;
    HWND window = CreateWindowW(cls.lpszClassName, L"menu probe", WS_OVERLAPPED, 0, 0, 320, 240, NULL, NULL, cls.hInstance, NULL);
    if (!window) return 2;
    templates(); tables(); translate(window); lifetime(window);
    if (!UnregisterClassW(cls.lpszClassName, cls.hInstance)) return 3;
    return 0;
}
