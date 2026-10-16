#define WIN32_LEAN_AND_MEAN
#define COBJMACROS
#include <windows.h>
#include <oleauto.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

static int row_count;
static void row(const char *name) {
    if (row_count++) putchar(',');
    printf("{\"case\":\"%s\"", name);
}
static void raw(const void *value, size_t length) {
    const unsigned char *bytes = (const unsigned char *)value;
    size_t i;
    putchar('[');
    for (i = 0; i < length; ++i) {
        if (i) putchar(',');
        printf("%u", (unsigned)bytes[i]);
    }
    putchar(']');
}
static void bstr(const char *name, BSTR value) {
    row(name);
    printf(",\"present\":%u,\"units\":%u,\"bytes\":%u,\"payload\":",
        value != NULL, SysStringLen(value), SysStringByteLen(value));
    raw(value, SysStringByteLen(value));
    putchar('}');
}
static void strings(void) {
    const WCHAR embedded[] = { 'a', 0, 'z' };
    const char odd[] = { (char)0xab, (char)0xcd, (char)0xef };
    BSTR value;
    BOOL ok;
    value = SysAllocString(NULL);
    bstr("alloc-null", value); SysFreeString(value);
    value = SysAllocStringLen(NULL, 0);
    bstr("alloc-empty", value); SysFreeString(value);
    value = SysAllocStringLen(embedded, 3);
    bstr("embedded-null", value);
    ok = SysReAllocStringLen(&value, value + 2, 1);
    row("realloc-alias"); printf(",\"ok\":%u,\"units\":%u,\"payload\":", ok, SysStringLen(value));
    raw(value, SysStringByteLen(value)); putchar('}');
    ok = SysReAllocString(&value, NULL);
    row("realloc-null"); printf(",\"ok\":%u,\"present\":%u}", ok, value != NULL);
    SysFreeString(value);
    value = SysAllocStringByteLen(odd, 3);
    bstr("binary-odd", value); SysFreeString(value);
}
static void variant_result(const char *name, HRESULT result, const VARIANT *value) {
    row(name);
    printf(",\"hr\":%lu,\"vt\":%u,\"value\":", (unsigned long)result, V_VT(value));
    {
        VARIANT normalized = *value;
        VARTYPE base = V_VT(value) & VT_TYPEMASK;
        if ((V_VT(value) & (VT_ARRAY | VT_BYREF)) || base == VT_BSTR || base == VT_UNKNOWN || base == VT_DISPATCH) {
            printf("null");
            printf(",\"pointerPresent\":%u,\"normalized\":", V_BYREF(value) != NULL);
            V_BYREF(&normalized) = NULL;
            raw(&normalized, sizeof(normalized));
        } else raw(value, sizeof(*value));
    }
    if (SUCCEEDED(result) && V_VT(value) == VT_BSTR) {
        printf(",\"text\":"); raw(V_BSTR(value), SysStringByteLen(V_BSTR(value)));
    }
    putchar('}');
}
static void variants(void) {
    static const double numbers[] = { 2.5, 3.5, -2.5, -3.5, 2147483647.5, -2147483648.5 };
    static const struct { const char *name; LCID locale; const WCHAR *text; VARTYPE type; } text[] = {
        { "german-decimal", 0x407, L"1,5", VT_R8 },
        { "german-grouped", 0x407, L"1.234,5", VT_R8 },
        { "english-comma", 0x409, L"1,5", VT_R8 },
        { "maximum-ui8", 0x409, L"18446744073709551615", VT_UI8 },
        { "overflow-ui8", 0x409, L"18446744073709551616", VT_UI8 },
        { "hebrew-numeric", 0x40d, L"123.5", VT_R8 },
    };
    unsigned i;
    VARIANT source, destination;
    HRESULT result;
    memset(&source, 0xa5, sizeof(source));
    VariantInit(&source);
    variant_result("init-sentinel", S_OK, &source);
    for (i = 0; i < sizeof(numbers)/sizeof(numbers[0]); ++i) {
        char name[64];
        VariantInit(&source); VariantInit(&destination);
        V_VT(&source) = VT_R8; V_R8(&source) = numbers[i];
        result = VariantChangeTypeEx(&destination, &source, 0x409, 0, VT_I4);
        sprintf_s(name, sizeof(name), "r8-to-i4-%u", i);
        variant_result(name, result, &destination);
        VariantClear(&destination);
    }
    for (i = 0; i < sizeof(text)/sizeof(text[0]); ++i) {
        VariantInit(&source); VariantInit(&destination);
        V_VT(&source) = VT_BSTR; V_BSTR(&source) = SysAllocString(text[i].text);
        result = VariantChangeTypeEx(&destination, &source, text[i].locale, 0, text[i].type);
        variant_result(text[i].name, result, &destination);
        VariantClear(&source); VariantClear(&destination);
    }
    for (i = 0; i < 4; ++i) {
        const WORD flags[] = { 0, VARIANT_ALPHABOOL, VARIANT_LOCALBOOL, VARIANT_ALPHABOOL | VARIANT_LOCALBOOL };
        char name[64];
        VariantInit(&source); VariantInit(&destination);
        V_VT(&source) = VT_BOOL; V_BOOL(&source) = VARIANT_TRUE;
        result = VariantChangeTypeEx(&destination, &source, 0x407, flags[i], VT_BSTR);
        sprintf_s(name, sizeof(name), "bool-to-german-string-%u", i);
        variant_result(name, result, &destination); VariantClear(&destination);
    }
    VariantInit(&source); VariantInit(&destination);
    V_VT(&source) = VT_NULL;
    result = VariantChangeTypeEx(&destination, &source, 0x409, 0, VT_I4);
    variant_result("null-to-i4", result, &destination);
    V_VT(&source) = VT_EMPTY;
    result = VariantChangeTypeEx(&destination, &source, 0x409, 0, VT_I4);
    variant_result("empty-to-i4", result, &destination);
    V_VT(&source) = VT_I4; V_I4(&source) = 7;
    result = VariantClear(&source);
    variant_result("clear-i4", result, &source);
    VariantClear(&destination);
}
static BOOL CALLBACK localized_boolean_locale(LPWSTR name) {
    LCID locale = (LCID)wcstoul(name, NULL, 16);
    unsigned truth;
    for (truth = 0; truth < 2; ++truth) {
        VARIANT source, result;
        HRESULT status;
        VariantInit(&source); VariantInit(&result);
        V_VT(&source) = VT_BOOL; V_BOOL(&source) = truth ? VARIANT_TRUE : VARIANT_FALSE;
        status = VariantChangeTypeEx(&result, &source, locale, VARIANT_LOCALBOOL, VT_BSTR);
        row("localized-boolean");
        printf(",\"locale\":%lu,\"truth\":%u,\"hr\":%lu,\"text\":", (unsigned long)locale, truth, (unsigned long)status);
        if (SUCCEEDED(status)) raw(V_BSTR(&result), SysStringByteLen(V_BSTR(&result))); else printf("null");
        putchar('}'); VariantClear(&result);
    }
    return TRUE;
}
static void numeric_precision(void) {
    static const double values[] = { 0, 0.1, 1.1, 1.2345678901234567, 1e-28, 1e-29, 7.9228162514264338e28, 1e29, -1.5, -0.00005, 922337203685477.5 };
    static const VARTYPE targets[] = { VT_DECIMAL, VT_CY, VT_BSTR };
    unsigned i, kind, target;
    for (i = 0; i < sizeof(values)/sizeof(values[0]); ++i) {
        for (kind = 0; kind < 2; ++kind) {
            for (target = 0; target < sizeof(targets)/sizeof(targets[0]); ++target) {
                VARIANT source, destination;
                HRESULT result;
                char name[64];
                VariantInit(&source); VariantInit(&destination);
                if (kind) { V_VT(&source) = VT_R4; V_R4(&source) = (FLOAT)values[i]; }
                else { V_VT(&source) = VT_R8; V_R8(&source) = values[i]; }
                result = VariantChangeTypeEx(&destination, &source, 0x409, 0, targets[target]);
                sprintf_s(name, sizeof(name), "precision-%u-%u-%u", i, kind, targets[target]);
                variant_result(name, result, &destination); VariantClear(&destination);
            }
        }
    }
    EnumSystemLocalesW(localized_boolean_locale, LCID_INSTALLED);
}
static void descriptor(const char *name, SAFEARRAY *array) {
    UINT i;
    row(name);
    printf(",\"present\":%u", array != NULL);
    if (!array) { putchar('}'); return; }
    printf(",\"dimensions\":%u,\"features\":%u,\"elementBytes\":%lu,\"locks\":%lu,\"descriptorBounds\":[",
        array->cDims, array->fFeatures, (unsigned long)array->cbElements, (unsigned long)array->cLocks);
    for (i = 0; i < array->cDims; ++i) {
        if (i) putchar(',');
        printf("{\"count\":%lu,\"lower\":%ld}", (unsigned long)array->rgsabound[i].cElements, (long)array->rgsabound[i].lLbound);
    }
    printf("],\"apiBounds\":[");
    for (i = 0; i <= array->cDims + 1; ++i) {
        LONG lower = 0x12345678, upper = 0x12345678;
        HRESULT lo = SafeArrayGetLBound(array, i, &lower), hi = SafeArrayGetUBound(array, i, &upper);
        if (i) putchar(',');
        printf("{\"dimension\":%u,\"lowerHr\":%lu,\"upperHr\":%lu,\"lower\":%ld,\"upper\":%ld}",
            i, (unsigned long)lo, (unsigned long)hi, (long)lower, (long)upper);
    }
    printf("]}");
}
static void arrays(void) {
    SAFEARRAYBOUND bounds[2] = { { 2, -3 }, { 3, 7 } };
    SAFEARRAYBOUND grow = { 4, 11 };
    const LONG indices[][2] = { { 7, -3 }, { 8, -3 }, { 7, -2 }, { -3, 7 }, { -2, 7 }, { -3, 8 } };
    SAFEARRAY *array = SafeArrayCreate(VT_I4, 2, bounds);
    LONG *data = NULL;
    unsigned i;
    HRESULT result;
    descriptor("create-two-dimensional", array);
    if (array) {
        result = SafeArrayAccessData(array, (void **)&data);
        row("access-array"); printf(",\"hr\":%lu,\"locks\":%lu}", (unsigned long)result, (unsigned long)array->cLocks);
        if (SUCCEEDED(result)) { for (i = 0; i < 6; ++i) data[i] = (LONG)(100 + i); SafeArrayUnaccessData(array); }
        for (i = 0; i < sizeof(indices)/sizeof(indices[0]); ++i) {
            LONG value = 0x12345678;
            char name[64];
            result = SafeArrayGetElement(array, (LONG *)indices[i], &value);
            sprintf_s(name, sizeof(name), "get-element-%u", i);
            row(name); printf(",\"hr\":%lu,\"value\":%ld}", (unsigned long)result, (long)value);
        }
        result = SafeArrayRedim(array, &grow);
        row("redim-rightmost"); printf(",\"hr\":%lu}", (unsigned long)result);
        descriptor("after-redim", array);
        result = SafeArrayAccessData(array, (void **)&data);
        if (SUCCEEDED(result)) {
            size_t count = 1;
            for (i = 0; i < array->cDims; ++i) count *= array->rgsabound[i].cElements;
            row("redim-data"); printf(",\"data\":"); raw(data, count * sizeof(LONG)); putchar('}'); SafeArrayUnaccessData(array);
        }
        SafeArrayDestroy(array);
    }
    array = SafeArrayCreateVector(VT_I4, -2, 3);
    descriptor("vector-descriptor", array);
    if (array) {
        VARIANT holder;
        VariantInit(&holder); V_VT(&holder) = VT_ARRAY | VT_I4; V_ARRAY(&holder) = array;
        SafeArrayLock(array);
        result = VariantClear(&holder);
        variant_result("clear-locked-array", result, &holder);
        SafeArrayUnlock(array);
        result = SafeArrayRedim(array, &grow);
        row("redim-vector"); printf(",\"hr\":%lu}", (unsigned long)result);
        VariantClear(&holder);
    }
    array = SafeArrayCreateVector(VT_I4, 0, 0);
    descriptor("zero-element-vector", array);
    if (array) SafeArrayDestroy(array);
    array = SafeArrayCreate(VT_I4, 0, bounds);
    descriptor("zero-dimensions", array);
    if (array) SafeArrayDestroy(array);
    array = SafeArrayCreateVector(VT_EMPTY, 0, 1);
    descriptor("empty-element-type", array);
    if (array) SafeArrayDestroy(array);
    array = SafeArrayCreateVector(VT_DECIMAL, 0, 1);
    descriptor("decimal-element-type", array);
    if (array) SafeArrayDestroy(array);
    array = SafeArrayCreateVector(VT_BSTR, 0, 1);
    if (array) {
        LONG index = 0;
        BSTR source = SysAllocString(L"source"), copy = NULL;
        result = SafeArrayPutElement(array, &index, source);
        SysFreeString(source);
        row("put-bstr"); printf(",\"hr\":%lu}", (unsigned long)result);
        result = SafeArrayGetElement(array, &index, &copy);
        row("get-bstr"); printf(",\"hr\":%lu,\"units\":%u,\"payload\":", (unsigned long)result, SysStringLen(copy)); raw(copy, SysStringByteLen(copy)); putchar('}');
        SysFreeString(copy); SafeArrayDestroy(array);
    }
}

typedef struct {
    IUnknown unknown;
    ULONG references;
    ULONG additions;
    ULONG releases;
} CountedUnknown;
static ULONG STDMETHODCALLTYPE counted_addref(IUnknown *self) {
    CountedUnknown *value = (CountedUnknown *)self;
    ++value->additions;
    return ++value->references;
}
static ULONG STDMETHODCALLTYPE counted_release(IUnknown *self) {
    CountedUnknown *value = (CountedUnknown *)self;
    ++value->releases;
    return --value->references;
}
static HRESULT STDMETHODCALLTYPE counted_query(IUnknown *self, REFIID iid, void **out) {
    static const GUID unknown = { 0, 0, 0, { 0xc0, 0, 0, 0, 0, 0, 0, 0x46 } };
    if (!out) return E_POINTER;
    *out = NULL;
    if (memcmp(iid, &unknown, sizeof(unknown)) != 0) return E_NOINTERFACE;
    *out = self; counted_addref(self); return S_OK;
}
static const IUnknownVtbl counted_vtable = { counted_query, counted_addref, counted_release };
static void reference_row(const char *name, HRESULT result, CountedUnknown *value) {
    row(name);
    printf(",\"hr\":%lu,\"references\":%lu,\"additions\":%lu,\"releases\":%lu}",
        (unsigned long)result, (unsigned long)value->references, (unsigned long)value->additions, (unsigned long)value->releases);
}
static void references(void) {
    CountedUnknown value = { { &counted_vtable }, 1, 0, 0 };
    IUnknown *pointer = &value.unknown, *copy = NULL;
    VARIANT source, destination;
    SAFEARRAY *array;
    LONG index = 0;
    HRESULT result;
    VariantInit(&source); VariantInit(&destination);
    V_VT(&source) = VT_UNKNOWN; V_UNKNOWN(&source) = pointer;
    result = VariantCopy(&destination, &source);
    reference_row("copy-unknown", result, &value);
    result = VariantClear(&destination);
    reference_row("clear-copied-unknown", result, &value);
    V_VT(&source) = VT_UNKNOWN | VT_BYREF; V_BYREF(&source) = &pointer;
    result = VariantCopy(&destination, &source);
    reference_row("copy-byref-unknown", result, &value);
    VariantClear(&destination);
    result = VariantCopyInd(&destination, &source);
    reference_row("copyind-byref-unknown", result, &value);
    VariantClear(&destination);
    VariantClear(&source);
    array = SafeArrayCreateVector(VT_UNKNOWN, 0, 1);
    if (array) {
        result = SafeArrayPutElement(array, &index, pointer);
        reference_row("put-unknown", result, &value);
        result = SafeArrayGetElement(array, &index, &copy);
        reference_row("get-unknown", result, &value);
        if (copy) IUnknown_Release(copy);
        result = SafeArrayDestroy(array);
        reference_row("destroy-unknown-array", result, &value);
    }
}

static void ordinals(void) {
    static const char *names[] = {
        "SysAllocString", "SysAllocStringLen", "SysAllocStringByteLen", "SysReAllocString", "SysReAllocStringLen", "SysFreeString", "SysStringLen", "SysStringByteLen",
        "VariantInit", "VariantClear", "VariantCopy", "VariantCopyInd", "VariantChangeType", "VariantChangeTypeEx",
        "SafeArrayCreate", "SafeArrayCreateVector", "SafeArrayDestroy", "SafeArrayGetDim", "SafeArrayGetElemsize", "SafeArrayGetLBound", "SafeArrayGetUBound",
        "SafeArrayLock", "SafeArrayUnlock", "SafeArrayAccessData", "SafeArrayUnaccessData", "SafeArrayGetElement", "SafeArrayPutElement", "SafeArrayCopy", "SafeArrayRedim", "SafeArrayGetVartype"
    };
    HMODULE module = GetModuleHandleW(L"oleaut32.dll");
    unsigned i, ordinal;
    for (i = 0; i < sizeof(names)/sizeof(names[0]); ++i) {
        FARPROC named = GetProcAddress(module, names[i]);
        row("export-ordinal");
        printf(",\"name\":\"%s\",\"present\":%u,\"ordinals\":[", names[i], named != NULL);
        {
            int count = 0;
            for (ordinal = 1; named && ordinal <= 512; ++ordinal) {
                if (GetProcAddress(module, MAKEINTRESOURCEA(ordinal)) == named) {
                    if (count++) putchar(',');
                    printf("%u", ordinal);
                }
            }
        }
        printf("]}");
    }
}

static void boolean_numeric_and_null_strings(void) {
    static const SHORT booleans[] = { 0, -1, 1, 2, -2, -32768, 32767 };
    static const VARTYPE targets[] = {
        VT_I1, VT_UI1, VT_I2, VT_UI2, VT_I4, VT_UI4, VT_I8, VT_UI8,
        VT_INT, VT_UINT, VT_R4, VT_R8, VT_CY, VT_DECIMAL
    };
    const WCHAR embedded[] = { 'a', 0, 'z' };
    const char odd[] = { (char)0xab, (char)0xcd, (char)0xef };
    unsigned i, target;
    for (i = 0; i < sizeof(booleans) / sizeof(booleans[0]); ++i) {
        for (target = 0; target < sizeof(targets) / sizeof(targets[0]); ++target) {
            VARIANT source, destination;
            HRESULT result;
            VariantInit(&source); VariantInit(&destination);
            V_VT(&source) = VT_BOOL; V_BOOL(&source) = booleans[i];
            result = VariantChangeTypeEx(&destination, &source, 0x409, 0, targets[target]);
            row("boolean-to-number");
            printf(",\"source\":%d,\"target\":%u,\"hr\":%lu,\"vt\":%u,\"value\":",
                (int)booleans[i], (unsigned)targets[target], (unsigned long)result, V_VT(&destination));
            raw(&destination, sizeof(destination)); putchar('}');
            VariantClear(&destination);
        }
    }
    for (i = 0; i < 4; ++i) {
        BSTR value = NULL;
        VARIANT source, destination, indirect;
        SAFEARRAY *array, *copy = NULL;
        BSTR element = NULL;
        LONG index = 0;
        HRESULT result;
        char name[64];
        if (i == 1) value = SysAllocStringLen(NULL, 0);
        if (i == 2) value = SysAllocStringLen(embedded, 3);
        if (i == 3) value = SysAllocStringByteLen(odd, 3);
        sprintf_s(name, sizeof(name), "copy-bstr-source-%u", i);
        bstr(name, value);
        VariantInit(&source); VariantInit(&destination); VariantInit(&indirect);
        V_VT(&source) = VT_BSTR; V_BSTR(&source) = value;
        result = VariantCopy(&destination, &source);
        sprintf_s(name, sizeof(name), "copy-null-empty-bstr-%u", i);
        variant_result(name, result, &destination); VariantClear(&destination);
        V_VT(&indirect) = VT_BSTR | VT_BYREF; V_BYREF(&indirect) = &value;
        result = VariantCopyInd(&destination, &indirect);
        sprintf_s(name, sizeof(name), "copyind-null-empty-bstr-%u", i);
        variant_result(name, result, &destination); VariantClear(&destination);

        array = SafeArrayCreateVector(VT_BSTR, 0, 1);
        if (array) {
            result = SafeArrayGetElement(array, &index, &element);
            sprintf_s(name, sizeof(name), "fresh-bstr-array-get-%u", i);
            row(name); printf(",\"hr\":%lu,\"present\":%u}", (unsigned long)result, element != NULL);
            if (SUCCEEDED(result)) SysFreeString(element);
            element = NULL;
            result = SafeArrayPutElement(array, &index, value);
            sprintf_s(name, sizeof(name), "null-empty-bstr-array-put-%u", i);
            row(name); printf(",\"hr\":%lu,\"storedPresent\":%u}", (unsigned long)result, ((BSTR *)array->pvData)[0] != NULL);
            VariantClear(&source); value = NULL;
            result = SafeArrayGetElement(array, &index, &element);
            sprintf_s(name, sizeof(name), "null-empty-bstr-array-get-%u", i);
            row(name);
            printf(",\"hr\":%lu,\"present\":%u,\"bytes\":%u,\"payload\":",
                (unsigned long)result, element != NULL, SysStringByteLen(element));
            raw(element, SysStringByteLen(element)); putchar('}');
            if (SUCCEEDED(result)) SysFreeString(element);
            element = NULL;
            result = SafeArrayCopy(array, &copy);
            sprintf_s(name, sizeof(name), "null-empty-bstr-array-copy-%u", i);
            row(name);
            printf(",\"hr\":%lu,\"present\":%u,\"storedPresent\":%u}",
                (unsigned long)result, copy != NULL, copy != NULL && ((BSTR *)copy->pvData)[0] != NULL);
            SafeArrayDestroy(array);
            if (copy) {
                result = SafeArrayGetElement(copy, &index, &element);
                sprintf_s(name, sizeof(name), "null-empty-bstr-array-copy-get-%u", i);
                row(name);
                printf(",\"hr\":%lu,\"present\":%u,\"bytes\":%u,\"payload\":",
                    (unsigned long)result, element != NULL, SysStringByteLen(element));
                raw(element, SysStringByteLen(element)); putchar('}');
                if (SUCCEEDED(result)) SysFreeString(element);
                SafeArrayDestroy(copy);
            }
        } else VariantClear(&source);
    }
}

static void numeric_text_boundaries_and_dates(void) {
    static const double numbers[] = {
        -0.0, 0.0001, 0.00001, 0.000001, 0.0000001, 0.00000001,
        -0.0001, -0.00001, -0.000001, -0.0000001, -0.00000001,
        999999, 1000000, 10000000, 999999999999999.0, 1000000000000000.0,
        9.999999499999999e-5, 9.999999999999995e-5,
        1.234567e-5, 1.234567e-7, 1.234567e-8,
        1.23456789012345e-8, 1.23456789012345e-14, 1.23456789012345e-15
    };
    static const DATE dates[] = { 0, 1.5, -1.25, 36925.0, 36925.54309027778 };
    static const WCHAR *date_text[] = {
        L"2/3/2001", L"03.02.2001", L"2001-02-03", L"1:02:03 PM",
        L"12:00 AM", L"invalid"
    };
    static const LCID locales[] = { 0x409, 0x407 };
    unsigned index, single, locale;
    for (index = 0; index < sizeof(numbers) / sizeof(numbers[0]); ++index) {
        for (single = 0; single < 2; ++single) {
            VARIANT source, destination;
            HRESULT result;
            char name[64];
            VariantInit(&source); VariantInit(&destination);
            if (single) { V_VT(&source) = VT_R4; V_R4(&source) = (FLOAT)numbers[index]; }
            else { V_VT(&source) = VT_R8; V_R8(&source) = numbers[index]; }
            result = VariantChangeTypeEx(&destination, &source, 0x409, 0, VT_BSTR);
            sprintf_s(name, sizeof(name), "numeric-text-boundary-%u-%u", index, single);
            variant_result(name, result, &destination); VariantClear(&destination);
        }
    }
    for (locale = 0; locale < sizeof(locales) / sizeof(locales[0]); ++locale) {
        for (index = 0; index < sizeof(dates) / sizeof(dates[0]); ++index) {
            VARIANT source, destination;
            HRESULT result;
            char name[64];
            VariantInit(&source); VariantInit(&destination);
            V_VT(&source) = VT_DATE; V_DATE(&source) = dates[index];
            result = VariantChangeTypeEx(&destination, &source, locales[locale], 0, VT_BSTR);
            sprintf_s(name, sizeof(name), "date-to-text-%u-%u", locale, index);
            variant_result(name, result, &destination); VariantClear(&destination);
        }
        for (index = 0; index < sizeof(date_text) / sizeof(date_text[0]); ++index) {
            VARIANT source, destination;
            HRESULT result;
            char name[64];
            VariantInit(&source); VariantInit(&destination);
            V_VT(&source) = VT_BSTR; V_BSTR(&source) = SysAllocString(date_text[index]);
            result = VariantChangeTypeEx(&destination, &source, locales[locale], 0, VT_DATE);
            sprintf_s(name, sizeof(name), "text-to-date-%u-%u", locale, index);
            variant_result(name, result, &destination);
            VariantClear(&source); VariantClear(&destination);
        }
    }
}

static void empty_null_conversions(void) {
    static const VARTYPE targets[] = {
        VT_EMPTY, VT_NULL, VT_I4, VT_R8, VT_BOOL, VT_BSTR, VT_DATE, VT_DECIMAL, VT_UNKNOWN
    };
    unsigned source_kind, target;
    for (source_kind = VT_EMPTY; source_kind <= VT_NULL; ++source_kind) {
        for (target = 0; target < sizeof(targets) / sizeof(targets[0]); ++target) {
            VARIANT source, destination;
            HRESULT result;
            char name[64];
            VariantInit(&source); VariantInit(&destination);
            V_VT(&source) = (VARTYPE)source_kind;
            result = VariantChangeTypeEx(&destination, &source, 0x409, 0, targets[target]);
            sprintf_s(name, sizeof(name), "empty-null-conversion-%u-%u", source_kind, targets[target]);
            variant_result(name, result, &destination); VariantClear(&destination);
        }
    }
}

typedef struct {
    IDispatch dispatch;
    ULONG references, additions, releases, queries, invocations;
    unsigned mode;
    DISPID member;
    LCID locale;
    WORD flags;
    UINT arguments, named;
    unsigned null_iid, parameters_present, result_present, exception_present, error_present;
} CountedDispatch;

static const GUID dispatch_unknown_iid = { 0, 0, 0, { 0xc0, 0, 0, 0, 0, 0, 0, 0x46 } };
static const GUID dispatch_iid = { 0x00020400, 0, 0, { 0xc0, 0, 0, 0, 0, 0, 0, 0x46 } };
static const GUID dispatch_null_iid;

static ULONG STDMETHODCALLTYPE dispatch_addref(IDispatch *self) {
    CountedDispatch *value = (CountedDispatch *)self;
    ++value->additions;
    return ++value->references;
}
static ULONG STDMETHODCALLTYPE dispatch_release(IDispatch *self) {
    CountedDispatch *value = (CountedDispatch *)self;
    ++value->releases;
    return --value->references;
}
static HRESULT STDMETHODCALLTYPE dispatch_query(IDispatch *self, REFIID iid, void **output) {
    CountedDispatch *value = (CountedDispatch *)self;
    ++value->queries;
    if (!output) return E_POINTER;
    *output = NULL;
    if (memcmp(iid, &dispatch_unknown_iid, sizeof(GUID)) != 0 &&
        (value->mode == 4 || memcmp(iid, &dispatch_iid, sizeof(GUID)) != 0)) return E_NOINTERFACE;
    *output = self;
    dispatch_addref(self);
    return S_OK;
}
static HRESULT STDMETHODCALLTYPE dispatch_type_count(IDispatch *self, UINT *output) {
    (void)self;
    if (!output) return E_POINTER;
    *output = 0;
    return S_OK;
}
static HRESULT STDMETHODCALLTYPE dispatch_type_info(IDispatch *self, UINT index, LCID locale, ITypeInfo **output) {
    (void)self; (void)index; (void)locale;
    if (output) *output = NULL;
    return E_NOTIMPL;
}
static HRESULT STDMETHODCALLTYPE dispatch_names(IDispatch *self, REFIID iid, LPOLESTR *names,
    UINT count, LCID locale, DISPID *output) {
    (void)self; (void)iid; (void)names; (void)count; (void)locale; (void)output;
    return DISP_E_UNKNOWNNAME;
}
static HRESULT STDMETHODCALLTYPE dispatch_invoke(IDispatch *self, DISPID member, REFIID iid,
    LCID locale, WORD flags, DISPPARAMS *parameters, VARIANT *result,
    EXCEPINFO *exception, UINT *error) {
    CountedDispatch *value = (CountedDispatch *)self;
    const WCHAR embedded[] = { 'a', 0, 'z' };
    ++value->invocations;
    value->member = member; value->locale = locale; value->flags = flags;
    value->null_iid = iid != NULL && memcmp(iid, &dispatch_null_iid, sizeof(GUID)) == 0;
    value->parameters_present = parameters != NULL;
    value->result_present = result != NULL;
    value->exception_present = exception != NULL;
    value->error_present = error != NULL;
    value->arguments = parameters ? parameters->cArgs : 0xffffffff;
    value->named = parameters ? parameters->cNamedArgs : 0xffffffff;
    if (value->mode == 2) return DISP_E_UNKNOWNNAME;
    if (member != DISPID_VALUE) return DISP_E_MEMBERNOTFOUND;
    if (!result) return E_POINTER;
    VariantInit(result);
    if (value->mode == 5 || value->mode == 6) {
        V_VT(result) = VT_UNKNOWN;
        V_UNKNOWN(result) = (IUnknown *)self;
        dispatch_addref(self);
        if (value->mode == 5) return DISP_E_UNKNOWNNAME;
    } else if (value->mode == 1) {
        V_VT(result) = VT_BSTR;
        V_BSTR(result) = SysAllocStringLen(embedded, 3);
        if (!V_BSTR(result)) return E_OUTOFMEMORY;
    } else {
        V_VT(result) = VT_I4;
        V_I4(result) = 37;
    }
    return S_OK;
}
static const IDispatchVtbl dispatch_vtable = {
    dispatch_query, dispatch_addref, dispatch_release, dispatch_type_count,
    dispatch_type_info, dispatch_names, dispatch_invoke
};
static void dispatch_state(const char *name, unsigned index, const CountedDispatch *value) {
    row(name);
    printf(",\"index\":%u,\"references\":%lu,\"additions\":%lu,\"releases\":%lu,\"queries\":%lu,\"invocations\":%lu",
        index, (unsigned long)value->references, (unsigned long)value->additions,
        (unsigned long)value->releases, (unsigned long)value->queries, (unsigned long)value->invocations);
    printf(",\"member\":%ld,\"locale\":%lu,\"flags\":%u,\"nullIid\":%u,\"parametersPresent\":%u,\"resultPresent\":%u,\"exceptionPresent\":%u,\"errorPresent\":%u,\"arguments\":%u,\"named\":%u}",
        (long)value->member, (unsigned long)value->locale, (unsigned)value->flags,
        value->null_iid, value->parameters_present, value->result_present,
        value->exception_present, value->error_present, value->arguments, value->named);
}
static void dispatch_conversions(void) {
    static const struct { VARTYPE source, target; USHORT flags; unsigned mode; } cases[] = {
        { VT_DISPATCH, VT_I4, 0, 0 },
        { VT_DISPATCH, VT_I4, VARIANT_NOVALUEPROP, 0 },
        { VT_DISPATCH, VT_BSTR, 0, 1 },
        { VT_DISPATCH, VT_I4, 0, 2 },
        { VT_UNKNOWN, VT_I4, 0, 0 },
        { VT_UNKNOWN, VT_DISPATCH, 0, 0 },
        { VT_DISPATCH, VT_UNKNOWN, 0, 0 },
        { VT_UNKNOWN, VT_DISPATCH, 0, 4 },
        { VT_DISPATCH, VT_DISPATCH, 0, 0 },
        { VT_DISPATCH, VT_I4, 0, 5 },
        { VT_DISPATCH, VT_I4, 0, 6 }
    };
    unsigned index;
    for (index = 0; index < sizeof(cases) / sizeof(cases[0]); ++index) {
        CountedDispatch value;
        VARIANT source, destination;
        HRESULT result;
        char name[64];
        memset(&value, 0, sizeof(value));
        value.dispatch.lpVtbl = &dispatch_vtable; value.references = 1; value.mode = cases[index].mode;
        VariantInit(&source); VariantInit(&destination);
        V_VT(&source) = cases[index].source;
        V_BYREF(&source) = &value.dispatch;
        result = VariantChangeTypeEx(&destination, &source, 0x407, cases[index].flags, cases[index].target);
        sprintf_s(name, sizeof(name), "dispatch-conversion-%u", index);
        variant_result(name, result, &destination);
        dispatch_state("dispatch-conversion-before-clear", index, &value);
        VariantClear(&destination); VariantClear(&source);
        dispatch_state("dispatch-conversion-after-clear", index, &value);
    }
}

int main(void) {
    printf("{\"schema\":1,\"pointerBytes\":%u,\"variantBytes\":%u,\"rows\":[", (unsigned)sizeof(void *), (unsigned)sizeof(VARIANT));
    strings(); variants(); arrays(); references(); ordinals(); numeric_precision(); boolean_numeric_and_null_strings(); numeric_text_boundaries_and_dates(); empty_null_conversions(); dispatch_conversions();
    printf("]}\n");
    return 0;
}
