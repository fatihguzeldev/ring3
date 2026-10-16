#define WIN32_LEAN_AND_MEAN
#define COBJMACROS
#include <windows.h>
#include <oleauto.h>
#include <stdio.h>
#include <string.h>

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

int main(void) {
    printf("{\"schema\":1,\"pointerBytes\":%u,\"variantBytes\":%u,\"rows\":[", (unsigned)sizeof(void *), (unsigned)sizeof(VARIANT));
    strings(); variants(); arrays(); references(); ordinals();
    printf("]}\n");
    return 0;
}
