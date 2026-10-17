#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <commctrl.h>
#include <stdio.h>
#include <string.h>

static void progress_row(HWND progress, const char *operation, LRESULT returned) {
    PBRANGE range = { 0, 0 };
    LRESULT bound = SendMessageW(progress, PBM_GETRANGE, TRUE, (LPARAM)&range);
    printf("{\"operation\":\"comctl-progress\",\"step\":\"%s\",\"returned\":%ld,\"low\":%d,\"high\":%d,\"bound\":%ld,\"position\":%ld}\n",
           operation, (long)returned, range.iLow, range.iHigh, (long)bound,
           (long)SendMessageW(progress, PBM_GETPOS, 0, 0));
}

int main(void) {
    INITCOMMONCONTROLSEX init = { sizeof(init), ICC_PROGRESS_CLASS };
    if (!InitCommonControlsEx(&init)) return 1;
    HWND owner = CreateWindowW(L"STATIC", L"progress-owner", WS_OVERLAPPEDWINDOW,
                               0, 0, 180, 100, NULL, NULL, GetModuleHandleW(NULL), NULL);
    HWND progress = owner ? CreateWindowW(PROGRESS_CLASSW, NULL, WS_CHILD | WS_VISIBLE,
                                          0, 0, 100, 20, owner, (HMENU)(ULONG_PTR)37,
                                          GetModuleHandleW(NULL), NULL) : NULL;
    if (!progress) { if (owner) DestroyWindow(owner); return 2; }
    progress_row(progress, "created", 0);
    progress_row(progress, "range16", SendMessageW(progress, PBM_SETRANGE, 0, MAKELPARAM(10, 20)));
    progress_row(progress, "positionBelow", SendMessageW(progress, PBM_SETPOS, (WPARAM)-5, 0));
    progress_row(progress, "positionInside", SendMessageW(progress, PBM_SETPOS, 17, 0));
    progress_row(progress, "positionAbove", SendMessageW(progress, PBM_SETPOS, 30, 0));
    progress_row(progress, "stepSet", SendMessageW(progress, PBM_SETSTEP, 7, 0));
    progress_row(progress, "stepWrap", SendMessageW(progress, PBM_STEPIT, 0, 0));
    progress_row(progress, "deltaBelow", SendMessageW(progress, PBM_DELTAPOS, (WPARAM)-40, 0));
    progress_row(progress, "negativeStep", SendMessageW(progress, PBM_SETSTEP, (WPARAM)-3, 0));
    progress_row(progress, "negativeWrap", SendMessageW(progress, PBM_STEPIT, 0, 0));
    progress_row(progress, "range32", SendMessageW(progress, PBM_SETRANGE32, (WPARAM)-200000, 300000));
    progress_row(progress, "positionWide", SendMessageW(progress, PBM_SETPOS, 123456, 0));
    progress_row(progress, "inverted", SendMessageW(progress, PBM_SETRANGE32, 50, 10));
    progress_row(progress, "equal", SendMessageW(progress, PBM_SETRANGE32, 7, 7));
    progress_row(progress, "full32", SendMessageW(progress, PBM_SETRANGE32, (WPARAM)0x80000000u, 0x7fffffff));
    progress_row(progress, "stepMaximum", SendMessageW(progress, PBM_SETSTEP, 0x7fffffff, 0));
    progress_row(progress, "positionMaximum", SendMessageW(progress, PBM_SETPOS, 0x7fffffff, 0));
    progress_row(progress, "overflowStep", SendMessageW(progress, PBM_STEPIT, 0, 0));
    progress_row(progress, "overflowDelta", SendMessageW(progress, PBM_DELTAPOS, 0x7fffffff, 0));
    progress_row(progress, "setBackground", SendMessageW(progress, PBM_SETBKCOLOR, 0, RGB(0x22, 0x44, 0x66)));
    progress_row(progress, "setBar", SendMessageW(progress, PBM_SETBARCOLOR, 0, RGB(0x88, 0xaa, 0xcc)));
    progress_row(progress, "defaultBackground", SendMessageW(progress, PBM_SETBKCOLOR, 0, CLR_DEFAULT));
    progress_row(progress, "defaultBar", SendMessageW(progress, PBM_SETBARCOLOR, 0, CLR_DEFAULT));
    DestroyWindow(owner);
    return 0;
}
