#include <windows.h>
#include <stdio.h>

_Static_assert(sizeof(void *) == 4, "this oracle requires a native 32-bit build");
_Static_assert(sizeof(WCHAR) == 2, "the oracle records UTF-16 code units");

static void probe_at(LCID locale, DWORD flags, unsigned pattern, const WCHAR *format,
                     const SYSTEMTIME *date) {
    WCHAR output[512] = {0};
    SetLastError(0);
    int required = GetDateFormatW(locale, flags, date, format, NULL, 0);
    DWORD required_error = GetLastError();
    SetLastError(0);
    int length = GetDateFormatW(locale, flags, date, format, output, 512);
    DWORD error = GetLastError();
    printf("RING3_NLS {\"lcid\":%lu,\"flags\":%lu,\"pattern\":%u,\"year\":%u,\"month\":%u,\"day\":%u,\"required\":%d,\"requiredError\":%lu,\"length\":%d,\"error\":%lu,\"units\":[",
           (unsigned long)locale, (unsigned long)flags, pattern,
           (unsigned)date->wYear, (unsigned)date->wMonth, (unsigned)date->wDay, required,
           (unsigned long)required_error, length, (unsigned long)error);
    if (length > 0 && length <= 512) {
        for (int index = 0; index < length - 1; ++index)
            printf("%s%u", index ? "," : "", (unsigned)output[index]);
    }
    puts("]}");
    DWORD locale_code_page = 0;
    if ((flags & LOCALE_USE_CP_ACP) || !GetLocaleInfoW(locale,
            LOCALE_IDEFAULTANSICODEPAGE | LOCALE_RETURN_NUMBER,
            (WCHAR *)&locale_code_page, sizeof(locale_code_page) / sizeof(WCHAR)))
        locale_code_page = 0;
    UINT code_page = locale_code_page ? locale_code_page : GetACP();
    char narrow_format[512] = {0};
    BOOL format_used_default = FALSE;
    int format_length = 0;
    DWORD format_error = 0;
    if (format) {
        SetLastError(0);
        format_length = WideCharToMultiByte(code_page, 0, format, -1,
            narrow_format, sizeof(narrow_format), NULL,
            code_page == CP_UTF8 || code_page == CP_UTF7 ? NULL : &format_used_default);
        format_error = GetLastError();
    }
    BOOL called = !format || format_length > 0;
    char bytes[512] = {0};
    SetLastError(0);
    length = called ? GetDateFormatA(locale, flags, date,
        format ? narrow_format : NULL, bytes, sizeof(bytes)) : 0;
    error = GetLastError();
    printf("RING3_NLS_ANSI {\"lcid\":%lu,\"flags\":%lu,\"pattern\":%u,\"year\":%u,\"month\":%u,\"day\":%u,\"codePage\":%u,\"called\":%s,\"formatLength\":%d,\"formatError\":%lu,\"formatUsedDefault\":%s,\"length\":%d,\"error\":%lu,\"formatBytes\":[",
           (unsigned long)locale, (unsigned long)flags, pattern,
           (unsigned)date->wYear, (unsigned)date->wMonth, (unsigned)date->wDay, code_page,
           called ? "true" : "false", format_length, (unsigned long)format_error,
           format_used_default ? "true" : "false", length, (unsigned long)error);
    for (int index = 0; index < format_length - 1; ++index)
        printf("%s%u", index ? "," : "", (unsigned)(unsigned char)narrow_format[index]);
    printf("],\"bytes\":[");
    if (length > 0 && length <= 512) {
        for (int index = 0; index < length - 1; ++index)
            printf("%s%u", index ? "," : "", (unsigned)(unsigned char)bytes[index]);
    }
    puts("]}");
}
static void probe(LCID locale, DWORD flags, unsigned pattern, const WCHAR *format) {
    const SYSTEMTIME date = {2001, 2, 0, 3, 4, 5, 6, 0};
    probe_at(locale, flags, pattern, format, &date);
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
static void direction_cases(void) {
    const LCID locales[] = {0x0409, 0x0401, 0x040d, 0x0420, 0x0429, 0x045a, 0x0465};
    const WCHAR *patterns[] = {
        L"yyyy", L"MMMM", L"dddd", L"gg", L"'ASCII'",
        L"'\x05e2\x05d1\x05e8\x05d9\x05ea'",
        L"'\x0627\x0644\x0639\x0631\x0628\x064a\x0629'",
        L"'ASCII' MMMM yyyy", L"MMMM 'ASCII' yyyy",
        L"yyyy''MM", L"yyyy-MM-dd", L"yyyy'---'MM''dd"
    };
    for (unsigned locale = 0; locale < sizeof(locales) / sizeof(locales[0]); ++locale) {
        metadata(locales[locale], 0, LOCALE_SNATIVEDIGITS);
        metadata(locales[locale], 0, LOCALE_IREADINGLAYOUT);
        metadata(locales[locale], 0, LOCALE_ICALENDARTYPE);
        metadata(locales[locale], 0, LOCALE_IOPTIONALCALENDAR);
        for (unsigned alternate = 0; alternate < 2; ++alternate) {
            DWORD calendar = alternate ? DATE_USE_ALT_CALENDAR : 0;
            for (unsigned pattern = 0; pattern < sizeof(patterns) / sizeof(patterns[0]); ++pattern) {
                probe(locales[locale], calendar, 0x101 + pattern, patterns[pattern]);
                probe(locales[locale], calendar | DATE_AUTOLAYOUT, 0x101 + pattern, patterns[pattern]);
                if ((locales[locale] == 0x045a || locales[locale] == 0x0465)
                        && (pattern == 0 || pattern == 4)) {
                    probe(locales[locale], calendar | DATE_LTRREADING, 0x101 + pattern, patterns[pattern]);
                    probe(locales[locale], calendar | DATE_RTLREADING, 0x101 + pattern, patterns[pattern]);
                }
            }
        }
        if (locales[locale] == 0x0409 || locales[locale] == 0x0401 || locales[locale] == 0x040d) {
            probe(locales[locale], LOCALE_USE_CP_ACP | DATE_LTRREADING, 0x201, L"yyyy/MM/dd");
            probe(locales[locale], LOCALE_USE_CP_ACP | DATE_RTLREADING, 0x201, L"yyyy/MM/dd");
        }
    }
}
static void hebrew_cases(void) {
    const SYSTEMTIME dates[] = {
        {1601, 1, 0, 1, 0, 0, 0, 0},
        {1700, 1, 0, 1, 0, 0, 0, 0},
        {1900, 1, 0, 1, 0, 0, 0, 0},
        {2000, 1, 0, 1, 0, 0, 0, 0},
        {2001, 2, 0, 3, 0, 0, 0, 0},
        {2024, 2, 0, 10, 0, 0, 0, 0},
        {2024, 3, 0, 11, 0, 0, 0, 0},
        {2024, 4, 0, 23, 0, 0, 0, 0},
        {2024, 4, 0, 24, 0, 0, 0, 0},
        {2025, 3, 0, 1, 0, 0, 0, 0},
        {2239, 9, 0, 30, 0, 0, 0, 0},
        {2240, 1, 0, 1, 0, 0, 0, 0},
        {9999, 12, 0, 31, 0, 0, 0, 0}
    };
    const WCHAR *patterns[] = {
        L"y", L"yy", L"yyy", L"yyyy", L"yyyyy",
        L"M", L"MM", L"MMM", L"MMMM",
        L"d", L"dd", L"ddd", L"dddd", L"g", L"gg"
    };
    for (unsigned date = 0; date < sizeof(dates) / sizeof(dates[0]); ++date) {
        for (unsigned pattern = 0; pattern < sizeof(patterns) / sizeof(patterns[0]); ++pattern) {
            probe_at(0x040d, DATE_USE_ALT_CALENDAR, 0x301 + pattern,
                     patterns[pattern], &dates[date]);
            probe_at(0x040d, DATE_USE_ALT_CALENDAR | DATE_AUTOLAYOUT, 0x301 + pattern,
                     patterns[pattern], &dates[date]);
        }
    }
}
static void hebrew_direction_classes(void) {
    const LCID locales[] = {0x040d, 0x0409};
    const WCHAR *patterns[] = {
        L"'\x05b0'", L"'\x05be'", L"'\x05c0'", L"'\x05c3'",
        L"'\x05f3'", L"'\xfb2a'", L"'\x07c0'", L"'\x200f'",
        L"'\x200e'", L"'A\x05b0'", L"'\x05d0\x05b0'", L"'\x0627\x05b0'"
    };
    for (unsigned locale = 0; locale < sizeof(locales) / sizeof(locales[0]); ++locale) {
        for (unsigned alternate = 0; alternate < 2; ++alternate) {
            DWORD calendar = alternate ? DATE_USE_ALT_CALENDAR : 0;
            for (unsigned pattern = 0; pattern < sizeof(patterns) / sizeof(patterns[0]); ++pattern) {
                probe(locales[locale], calendar, 0x401 + pattern, patterns[pattern]);
                probe(locales[locale], calendar | DATE_AUTOLAYOUT, 0x401 + pattern, patterns[pattern]);
            }
        }
    }
}
static void embedded_direction_marks(void) {
    const LCID locales[] = {0x040d, 0x0409};
    const DWORD layouts[] = {0, DATE_LTRREADING, DATE_RTLREADING, DATE_AUTOLAYOUT};
    const WCHAR *patterns[] = {
        L"'A\x200f'", L"'\x05d0\x200e'", L"'\x05d0\x200f'", L"'\x0627\x200e'"
    };
    for (unsigned locale = 0; locale < sizeof(locales) / sizeof(locales[0]); ++locale)
        for (unsigned layout = 0; layout < sizeof(layouts) / sizeof(layouts[0]); ++layout)
            for (unsigned pattern = 0; pattern < sizeof(patterns) / sizeof(patterns[0]); ++pattern)
                probe(locales[locale], layouts[layout], 0x501 + pattern, patterns[pattern]);
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
    direction_cases();
    hebrew_cases();
    hebrew_direction_classes();
    embedded_direction_marks();
    return 0;
}
