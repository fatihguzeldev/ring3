#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdarg.h>
#include <stdio.h>

static void emit(const char *name, DWORD result, DWORD error, const WCHAR *text) {
    printf("{\"case\":\"%s\",\"result\":%lu,\"error\":%lu,\"units\":[", name, result, error);
    for (DWORD i = 0; i < result; ++i) printf("%s%u", i ? "," : "", text[i]);
    puts("]}");
}
static void array(const char *name, DWORD flags, const WCHAR *text, DWORD_PTR *arguments) {
    printf("{\"begin\":\"%s\"}\n", name);
    WCHAR output[256] = {0}; SetLastError(0xabcdef01);
    DWORD result = 0, error = 0, exception = 0;
    __try {
        result = FormatMessageW(FORMAT_MESSAGE_FROM_STRING | flags, text, 0, 0, output, 256, (va_list *)arguments);
        error = GetLastError();
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        exception = GetExceptionCode();
    }
    if (exception) printf("{\"case\":\"%s\",\"exception\":%lu}\n", name, exception);
    else emit(name, result, error, output);
}
static void variable(const char *name, const WCHAR *text, ...) {
    printf("{\"begin\":\"%s\"}\n", name);
    WCHAR output[256] = {0}; va_list arguments; va_start(arguments, text); SetLastError(0xabcdef01);
    DWORD result = FormatMessageW(FORMAT_MESSAGE_FROM_STRING, text, 0, 0, output, 256, &arguments);
    emit(name, result, GetLastError(), output); va_end(arguments);
}
int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    array("escapes", 0, L"A\nB\rC\r\nD%nE%rF%tG%%%.%!%b%0tail", NULL);
    array("ignore", FORMAT_MESSAGE_IGNORE_INSERTS, L"[%1!08x!][%2]%%", NULL);
    array("ignore-escapes", FORMAT_MESSAGE_IGNORE_INSERTS, L"A%nB%rC%tD%%E%0tail", NULL);
    array("ignore-invalid-va", FORMAT_MESSAGE_IGNORE_INSERTS, L"[%1!08x!][%2]%%", (DWORD_PTR *)0xfffffff0);
    array("flatten", 255, L"a\r\nb\nc\rd%nend", NULL);
    array("wrap", 5, L"a bb ccc ddddddd e", NULL);
    DWORD_PTR stars[] = {4, 2, (DWORD_PTR)L"Ned", (DWORD_PTR)L"Sue", 6, (DWORD_PTR)L"Ned"};
    array("array-stars", FORMAT_MESSAGE_ARGUMENT_ARRAY, L"%1!*.*s!/%4/%5!*s!", stars);
    variable("variable-stars", L"%1!*.*s!/%3/%4!*s!", 4, 2, L"Ned", L"Sue", 6, L"Ned");
    variable("integer64", L"%1!I64x!/%2!d!/%3!d!", (unsigned __int64)0x1122334455667788, -7, 11, 22);
    DWORD_PTR integers[] = {(DWORD_PTR)-7, 0x1a, 8, (DWORD_PTR)L"Ned"};
    array("integer-flags", FORMAT_MESSAGE_ARGUMENT_ARRAY, L"%1!+06d!/%2!#08x!/%3!u!/%4!.2s!", integers);
    WCHAR output[4] = {0x1234, 0x2345, 0x3456, 0x4567}; SetLastError(0);
    DWORD result = FormatMessageW(FORMAT_MESSAGE_FROM_STRING, L"1234", 0, 0, output, 3, NULL);
    printf("{\"case\":\"small\",\"result\":%lu,\"error\":%lu,\"units\":[%u,%u,%u,%u]}\n", result, GetLastError(), output[0], output[1], output[2], output[3]);
    return 0;
}
