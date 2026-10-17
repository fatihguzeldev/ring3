#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <imm.h>
#include <stdio.h>
#include <string.h>

static void context_row(const char *stage, HIMC context) {
    DWORD conversion = 0xcccccccc, sentence = 0xcccccccc;
    COMPOSITIONFORM composition;
    CANDIDATEFORM candidate;
    LOGFONTW font;
    memset(&composition, 0xcc, sizeof(composition));
    memset(&candidate, 0xcc, sizeof(candidate));
    memset(&font, 0xcc, sizeof(font));
    BOOL converted = ImmGetConversionStatus(context, &conversion, &sentence);
    BOOL composed = ImmGetCompositionWindow(context, &composition);
    BOOL nominated = ImmGetCandidateWindow(context, 0, &candidate);
    BOOL font_read = ImmGetCompositionFontW(context, &font);
    printf("{\"operation\":\"imm-context\",\"stage\":\"%s\",\"present\":%d,\"open\":%d,"
           "\"conversionRead\":%d,\"conversion\":%lu,\"sentence\":%lu,"
           "\"compositionRead\":%d,\"composition\":[%lu,%ld,%ld,%ld,%ld,%ld,%ld],"
           "\"candidateRead\":%d,\"candidate\":[%lu,%lu,%ld,%ld,%ld,%ld,%ld,%ld],"
           "\"fontRead\":%d,\"font\":[%ld,%lu,%u],\"compositionBytes\":%ld,\"resultBytes\":%ld}\n",
           stage, context != NULL, ImmGetOpenStatus(context), converted,
           (unsigned long)conversion, (unsigned long)sentence, composed,
           (unsigned long)composition.dwStyle, (long)composition.ptCurrentPos.x,
           (long)composition.ptCurrentPos.y, (long)composition.rcArea.left,
           (long)composition.rcArea.top, (long)composition.rcArea.right, (long)composition.rcArea.bottom,
           nominated, (unsigned long)candidate.dwIndex, (unsigned long)candidate.dwStyle,
           (long)candidate.ptCurrentPos.x, (long)candidate.ptCurrentPos.y,
           (long)candidate.rcArea.left, (long)candidate.rcArea.top,
           (long)candidate.rcArea.right, (long)candidate.rcArea.bottom,
           font_read, (long)font.lfHeight, (unsigned long)font.lfWeight, (unsigned)font.lfCharSet,
           ImmGetCompositionStringW(context, GCS_COMPSTR, NULL, 0),
           ImmGetCompositionStringW(context, GCS_RESULTSTR, NULL, 0));
}

int main(void) {
    const WCHAR class_name[] = L"Ring3OriginalImmContextProbe";
    WNDCLASSW type;
    memset(&type, 0, sizeof(type));
    type.lpfnWndProc = DefWindowProcW;
    type.hInstance = GetModuleHandleW(NULL);
    type.lpszClassName = class_name;
    if (!RegisterClassW(&type)) return 1;
    HWND first = CreateWindowW(class_name, L"first", WS_OVERLAPPEDWINDOW,
        0, 0, 100, 80, NULL, NULL, type.hInstance, NULL);
    HWND second = CreateWindowW(class_name, L"second", WS_OVERLAPPEDWINDOW,
        0, 0, 100, 80, NULL, NULL, type.hInstance, NULL);
    if (!first || !second) return 2;
    HIMC first_default = ImmGetContext(first);
    HIMC second_default = ImmGetContext(second);
    HKL layout = GetKeyboardLayout(0);
    printf("{\"operation\":\"imm-defaults\",\"first\":%d,\"second\":%d,\"same\":%d,"
           "\"layout\":%lu,\"ime\":%d,\"defaultWindow\":%d}\n",
           first_default != NULL, second_default != NULL,
           first_default && first_default == second_default,
           (unsigned long)(ULONG_PTR)layout, ImmIsIME(layout),
           IsWindow(ImmGetDefaultIMEWnd(first)));
    if (first_default) ImmReleaseContext(first, first_default);
    if (second_default) ImmReleaseContext(second, second_default);
    HIMC custom = ImmCreateContext();
    printf("{\"operation\":\"imm-create\",\"created\":%d}\n", custom != NULL);
    if (custom) {
        context_row("created", custom);
        HIMC previous = ImmAssociateContext(first, custom);
        HIMC current = ImmGetContext(first);
        HIMC other = ImmGetContext(second);
        printf("{\"operation\":\"imm-associated\",\"previousDefault\":%d,\"currentCustom\":%d,\"otherUnchanged\":%d}\n",
               previous == first_default, current == custom, other == second_default);
        if (current) ImmReleaseContext(first, current);
        if (other) ImmReleaseContext(second, other);
        BOOL opened = ImmSetOpenStatus(custom, TRUE);
        BOOL converted = ImmSetConversionStatus(custom, IME_CMODE_NATIVE, IME_SMODE_NONE);
        COMPOSITIONFORM composition = { CFS_POINT, { 11, 17 }, { 2, 3, 97, 63 } };
        CANDIDATEFORM candidate = { 0, CFS_EXCLUDE, { 7, 9 }, { 2, 3, 47, 23 } };
        LOGFONTW font;
        memset(&font, 0, sizeof(font));
        font.lfHeight = -18;
        font.lfWeight = 400;
        font.lfCharSet = DEFAULT_CHARSET;
        lstrcpyW(font.lfFaceName, L"Arial");
        BOOL composed = ImmSetCompositionWindow(custom, &composition);
        BOOL nominated = ImmSetCandidateWindow(custom, &candidate);
        BOOL font_set = ImmSetCompositionFontW(custom, &font);
        printf("{\"operation\":\"imm-set\",\"open\":%d,\"conversion\":%d,\"composition\":%d,\"candidate\":%d,\"font\":%d}\n",
               opened, converted, composed, nominated, font_set);
        context_row("configured", custom);
        HIMC detached = ImmAssociateContext(first, NULL);
        HIMC absent = ImmGetContext(first);
        printf("{\"operation\":\"imm-detached\",\"previousCustom\":%d,\"absent\":%d}\n",
               detached == custom, absent == NULL);
        if (absent) ImmReleaseContext(first, absent);
        ImmAssociateContext(first, previous);
        context_row("unassociated", custom);
        printf("{\"operation\":\"imm-destroy\",\"destroyed\":%d}\n", ImmDestroyContext(custom));
    }
    DestroyWindow(second);
    DestroyWindow(first);
    if (!UnregisterClassW(class_name, type.hInstance)) return 3;
    return 0;
}
