#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

static void insertion(unsigned mode, unsigned position) {
    HMENU menu = CreateMenu();
    MENUITEMINFOW item;
    memset(&item, 0, sizeof(item));
    item.cbSize = sizeof(item);
    item.fMask = MIIM_ID | MIIM_STRING;
    item.wID = 37;
    item.dwTypeData = L"one";
    SetLastError(0);
    BOOL ok = mode < 2
        ? InsertMenuW(menu, position, mode ? MF_BYCOMMAND : MF_BYPOSITION, 37, L"one")
        : InsertMenuItemW(menu, position, mode == 2, &item);
    DWORD error = GetLastError();
    printf("{\"operation\":\"menu-insert-boundary\",\"mode\":%u,\"position\":%u,\"ok\":%d,\"error\":%lu,\"count\":%d}\n",
           mode, position, !!ok, (unsigned long)error, GetMenuItemCount(menu));
    DestroyMenu(menu);
}

static void state(unsigned flag) {
    HMENU menu = CreateMenu();
    AppendMenuW(menu, flag, 37, L"state");
    MENUITEMINFOW item;
    memset(&item, 0, sizeof(item));
    item.cbSize = sizeof(item);
    item.fMask = MIIM_FTYPE | MIIM_STATE;
    GetMenuItemInfoW(menu, 0, TRUE, &item);
    UINT legacy = GetMenuState(menu, 0, MF_BYPOSITION);
    UINT previous = EnableMenuItem(menu, 37, MF_ENABLED);
    printf("{\"operation\":\"menu-state-fields\",\"flag\":%u,\"type\":%u,\"state\":%u,\"legacy\":%u,\"previous\":%u}\n",
           flag, item.fType, item.fState, legacy, previous);
    DestroyMenu(menu);
}

static void information(unsigned count) {
    HMENU menu = CreateMenu();
    AppendMenuW(menu, MF_STRING, 37, L"abcdef");
    MENUITEMINFOW item;
    WCHAR text[16];
    memset(&item, 0, sizeof(item));
    for (unsigned i = 0; i != 16; ++i) text[i] = 0xeeee;
    item.cbSize = sizeof(item);
    item.fMask = MIIM_STRING;
    item.dwTypeData = text;
    item.cch = count;
    SetLastError(0);
    BOOL ok = GetMenuItemInfoW(menu, 0, TRUE, &item);
    DWORD error = GetLastError();
    printf("{\"operation\":\"menu-string-count\",\"capacity\":%u,\"ok\":%d,\"error\":%lu,\"count\":%u,\"text\":[%u,%u,%u,%u]}\n",
           count, !!ok, (unsigned long)error, item.cch, text[0], text[1], text[2], text[3]);
    DestroyMenu(menu);
}

static void owner_data(void) {
    HMENU menu = CreateMenu();
    SetLastError(0);
    BOOL appended = AppendMenuW(menu, MF_OWNERDRAW, 37, (LPCWSTR)(ULONG_PTR)0x12345678);
    DWORD append_error = GetLastError();
    MENUITEMINFOW item;
    memset(&item, 0, sizeof(item));
    item.cbSize = sizeof(item);
    item.fMask = MIIM_TYPE | MIIM_DATA;
    BOOL queried = GetMenuItemInfoW(menu, 0, TRUE, &item);
    printf("{\"operation\":\"menu-owner-data\",\"append\":%d,\"appendError\":%lu,\"query\":%d,\"type\":%u,\"data\":%lu,\"typeData\":%lu,\"count\":%u}\n",
           !!appended, (unsigned long)append_error, !!queried, item.fType,
           (unsigned long)item.dwItemData, (unsigned long)(ULONG_PTR)item.dwTypeData, item.cch);
    DestroyMenu(menu);
}

int main(void) {
    for (unsigned mode = 0; mode != 4; ++mode) {
        insertion(mode, 0); insertion(mode, 1); insertion(mode, 99); insertion(mode, 0xffffffffu);
    }
    state(MF_GRAYED); state(MF_DISABLED); state(MF_MENUBARBREAK); state(MF_MENUBREAK);
    information(0); information(1); information(3); information(16);
    owner_data();
    return 0;
}
