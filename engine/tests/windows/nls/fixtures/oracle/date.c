#include <windows.h>
#include <stdio.h>

_Static_assert(sizeof(void *) == 4, "this oracle requires a native 32-bit build");
_Static_assert(sizeof(WCHAR) == 2, "the oracle records UTF-16 code units");

static void probe(LCID locale, DWORD flags, unsigned pattern, const WCHAR *format) {
    const SYSTEMTIME date = {2001, 2, 0, 3, 4, 5, 6, 0};
    WCHAR output[512] = {0};
    SetLastError(0);
    int required = GetDateFormatW(locale, flags, &date, format, NULL, 0);
    DWORD required_error = GetLastError();
    SetLastError(0);
    int length = GetDateFormatW(locale, flags, &date, format, output, 512);
    DWORD error = GetLastError();
    printf("RING3_NLS {\"lcid\":%lu,\"flags\":%lu,\"pattern\":%u,\"required\":%d,\"requiredError\":%lu,\"length\":%d,\"error\":%lu,\"units\":[",
           (unsigned long)locale, (unsigned long)flags, pattern, required,
           (unsigned long)required_error, length, (unsigned long)error);
    if (length > 0 && length <= 512) {
        for (int index = 0; index < length - 1; ++index)
            printf("%s%u", index ? "," : "", (unsigned)output[index]);
    }
    puts("]}");
    char narrow_format[128] = {0};
    if (format) {
        for (unsigned index = 0; index < 127 && format[index]; ++index)
            narrow_format[index] = (char)format[index];
    }
    char bytes[512] = {0};
    SetLastError(0);
    length = GetDateFormatA(locale, flags, &date, format ? narrow_format : NULL, bytes, 512);
    error = GetLastError();
    printf("RING3_NLS_ANSI {\"lcid\":%lu,\"flags\":%lu,\"pattern\":%u,\"length\":%d,\"error\":%lu,\"bytes\":[",
           (unsigned long)locale, (unsigned long)flags, pattern, length, (unsigned long)error);
    if (length > 0 && length <= 512) {
        for (int index = 0; index < length - 1; ++index)
            printf("%s%u", index ? "," : "", (unsigned)(unsigned char)bytes[index]);
    }
    puts("]}");
}
static void metadata(LCID locale, CALID calendar, DWORD kind) {
    WCHAR output[512] = {0};
    SetLastError(0);
    int length = calendar
        ? GetCalendarInfoW(locale, calendar, kind | CAL_NOUSEROVERRIDE, output, 512, NULL)
        : GetLocaleInfoW(locale, kind | LOCALE_NOUSEROVERRIDE, output, 512);
    DWORD error = GetLastError();
    printf("RING3_PATTERN {\"lcid\":%lu,\"calendar\":%lu,\"type\":%lu,\"length\":%d,\"error\":%lu,\"units\":[",
           (unsigned long)locale, (unsigned long)calendar, (unsigned long)kind,
           length, (unsigned long)error);
    if (length > 0 && length <= 512) {
        for (int index = 0; index < length - 1; ++index)
            printf("%s%u", index ? "," : "", (unsigned)output[index]);
    }
    puts("]}");
}
int main(void) {
    const LCID locales[] = {0x0409, 0x0401, 0x0c01, 0x040d, 0x0429, 0x0420, 0x0411, 0x007f};
    const DWORD layouts[] = {0, DATE_LTRREADING, DATE_RTLREADING, DATE_AUTOLAYOUT};
    const DWORD styles[] = {DATE_SHORTDATE, DATE_LONGDATE, DATE_YEARMONTH};
    const DWORD locale_types[] = {LOCALE_SSHORTDATE, LOCALE_SLONGDATE, LOCALE_SYEARMONTH, LOCALE_ICALENDARTYPE, LOCALE_IOPTIONALCALENDAR};
    const DWORD calendar_types[] = {CAL_SSHORTDATE, CAL_SLONGDATE, CAL_SYEARMONTH, CAL_SCALNAME};
    const WCHAR *patterns[] = {L"yyyy/MM/dd", L"dd MMMM yyyy", L"'prefix' d-M-yyyy 'suffix'", L"yyyy'-'MM'-'dd"};
    for (unsigned locale = 0; locale < sizeof(locales) / sizeof(locales[0]); ++locale) {
        for (unsigned kind = 0; kind < sizeof(locale_types) / sizeof(locale_types[0]); ++kind)
            metadata(locales[locale], 0, locale_types[kind]);
        for (CALID calendar = 1; calendar <= 23; ++calendar)
            for (unsigned kind = 0; kind < sizeof(calendar_types) / sizeof(calendar_types[0]); ++kind)
                metadata(locales[locale], calendar, calendar_types[kind]);
        for (unsigned layout = 0; layout < sizeof(layouts) / sizeof(layouts[0]); ++layout) {
            for (unsigned style = 0; style < sizeof(styles) / sizeof(styles[0]); ++style) {
                probe(locales[locale], LOCALE_NOUSEROVERRIDE | styles[style] | layouts[layout], 0, NULL);
                probe(locales[locale], LOCALE_NOUSEROVERRIDE | styles[style] | layouts[layout] | DATE_USE_ALT_CALENDAR, 0, NULL);
            }
            for (unsigned pattern = 0; pattern < sizeof(patterns) / sizeof(patterns[0]); ++pattern)
                probe(locales[locale], layouts[layout], pattern + 1, patterns[pattern]);
        }
    }
    return 0;
}
