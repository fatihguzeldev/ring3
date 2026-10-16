#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shlwapi.h>
#include <stdio.h>
#include <wchar.h>

static int rows;
static void row(const char *name) {
    if (rows++) putchar(',');
    printf("{\"case\":\"%s\"", name);
}
static void comparison(const char *name, const WCHAR *left, const WCHAR *right) {
    row(name);
    printf(",\"userLocale\":%lu,\"threadLocale\":%lu,\"shell\":%d,\"shellPrefix1\":%d,\"userCompare\":%d,\"threadCompare\":%d,\"threadLinguisticCasing\":%d,\"threadLinguisticIgnoreCase\":%d}",
        (unsigned long)GetUserDefaultLCID(), (unsigned long)GetThreadLocale(),
        StrCmpIW(left,right), StrCmpNIW(left,right,1),
        CompareStringW(LOCALE_USER_DEFAULT,NORM_IGNORECASE,left,-1,right,-1)-CSTR_EQUAL,
        CompareStringW(GetThreadLocale(),NORM_IGNORECASE,left,-1,right,-1)-CSTR_EQUAL,
        CompareStringW(GetThreadLocale(),NORM_IGNORECASE|NORM_LINGUISTIC_CASING,left,-1,right,-1)-CSTR_EQUAL,
        CompareStringW(GetThreadLocale(),LINGUISTIC_IGNORECASE,left,-1,right,-1)-CSTR_EQUAL);
}
static void prefixes(void) {
    SYSTEM_INFO info;
    BYTE *memory;
    DWORD previous, fault;
    int result;
    GetSystemInfo(&info);
    memory=(BYTE *)VirtualAlloc(NULL,info.dwPageSize*4,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE);
    if (!memory) { row("prefix-allocation"); printf(",\"error\":%lu}",(unsigned long)GetLastError()); return; }
    VirtualProtect(memory+info.dwPageSize,info.dwPageSize,PAGE_NOACCESS,&previous);
    VirtualProtect(memory+info.dwPageSize*3,info.dwPageSize,PAGE_NOACCESS,&previous);
    result=1234; fault=0;
    __try { result=StrCmpNIW((LPCWSTR)(memory+info.dwPageSize),(LPCWSTR)(memory+info.dwPageSize*3),0); }
    __except(EXCEPTION_EXECUTE_HANDLER) { fault=GetExceptionCode(); }
    row("zero-inaccessible-wide"); printf(",\"result\":%d,\"fault\":%lu}",result,(unsigned long)fault);
    result=1234; fault=0;
    __try { result=StrCmpNIA((LPCSTR)(memory+info.dwPageSize),(LPCSTR)(memory+info.dwPageSize*3),0); }
    __except(EXCEPTION_EXECUTE_HANDLER) { fault=GetExceptionCode(); }
    row("zero-inaccessible-ansi"); printf(",\"result\":%d,\"fault\":%lu}",result,(unsigned long)fault);
    *(WCHAR *)(memory+info.dwPageSize-2)=L'A';
    *(WCHAR *)(memory+info.dwPageSize*3-2)=L'a';
    result=1234; fault=0;
    __try { result=StrCmpNIW((LPCWSTR)(memory+info.dwPageSize-2),(LPCWSTR)(memory+info.dwPageSize*3-2),1); }
    __except(EXCEPTION_EXECUTE_HANDLER) { fault=GetExceptionCode(); }
    row("wide-prefix-at-end"); printf(",\"result\":%d,\"fault\":%lu}",result,(unsigned long)fault);
    memory[info.dwPageSize-2]=0x83;memory[info.dwPageSize-1]=0x5c;
    memory[info.dwPageSize*3-2]=0x83;memory[info.dwPageSize*3-1]=0x5c;
    result=1234; fault=0;
    __try { result=StrCmpNIA((LPCSTR)(memory+info.dwPageSize-2),(LPCSTR)(memory+info.dwPageSize*3-2),2); }
    __except(EXCEPTION_EXECUTE_HANDLER) { fault=GetExceptionCode(); }
    row("ansi-two-byte-prefix-at-end"); printf(",\"result\":%d,\"fault\":%lu}",result,(unsigned long)fault);
    result=1234;fault=0;
    __try {result=StrCmpNIW(L"ab",L"ac",-1);}
    __except(EXCEPTION_EXECUTE_HANDLER) {fault=GetExceptionCode();}
    row("negative-count-wide");printf(",\"result\":%d,\"fault\":%lu}",result,(unsigned long)fault);
    VirtualFree(memory,0,MEM_RELEASE);
}
static void text(const WCHAR *value) {
    unsigned i;
    putchar('[');
    for (i=0;i<MAX_PATH && value[i];++i) {
        if (i) putchar(',');
        printf("%u",(unsigned)value[i]);
    }
    putchar(']');
}
static void paths(void) {
    static const WCHAR *inputs[]={L"",L".",L"..",L".\\a",L"..\\a",L"a\\..\\b",L"C:",L"C:foo",L"C:\\",L"C:\\foo\\",L"C:\\foo.bar space",L"\\\\server",L"\\\\server\\share",L"\\\\server\\share\\",L"a/b.txt",L"a..b",L"C:\\folder\\..\\target",L"C:\\..\\target"};
    static const struct {const WCHAR *directory;const WCHAR *file;} pairs[]={
        {L"C:\\one",L"\\two"},{L"C:\\one",L"\\\\two"},{L"C:\\one",L"D:\\two"},{L"C:\\one",L"..\\two"},{L"..\\one\\two",L"three"},{L"C:",L"x"},{L"",L"x"},{L"",L"\\x"}
    };
    unsigned i;
    for (i=0;i<sizeof(inputs)/sizeof(inputs[0]);++i) {
        const WCHAR *input=inputs[i];
        WCHAR output[MAX_PATH];
        WCHAR *pointer;
        BOOL result;
        row("path");printf(",\"input\":");text(input);
        printf(",\"nameOffset\":%ld,\"extensionOffset\":%ld,\"root\":%u,\"relative\":%u,\"unc\":%u",(long)(PathFindFileNameW(input)-input),(long)(PathFindExtensionW(input)-input),PathIsRootW(input),PathIsRelativeW(input),PathIsUNCW(input));
        output[0]=0;result=PathCanonicalizeW(output,input);printf(",\"canonicalOk\":%u,\"canonical\":",result);text(output);
        wcscpy_s(output,MAX_PATH,input);result=PathRemoveFileSpecW(output);printf(",\"removeFileOk\":%u,\"removeFile\":",result);text(output);
        wcscpy_s(output,MAX_PATH,input);result=PathStripToRootW(output);printf(",\"stripRootOk\":%u,\"stripRoot\":",result);text(output);
        wcscpy_s(output,MAX_PATH,input);pointer=PathAddBackslashW(output);printf(",\"addBackslashOffset\":%ld,\"addBackslash\":",pointer?(long)(pointer-output):-1L);text(output);
        wcscpy_s(output,MAX_PATH,input);pointer=PathRemoveBackslashW(output);printf(",\"removeBackslashOffset\":%ld,\"removeBackslash\":",pointer?(long)(pointer-output):-1L);text(output);
        putchar('}');
    }
    for (i=0;i<sizeof(pairs)/sizeof(pairs[0]);++i) {
        WCHAR output[MAX_PATH];
        BOOL appended;
        WCHAR *combined;
        row("join");printf(",\"directory\":");text(pairs[i].directory);printf(",\"file\":");text(pairs[i].file);
        output[0]=0;combined=PathCombineW(output,pairs[i].directory,pairs[i].file);printf(",\"combineOk\":%u,\"combined\":",combined!=NULL);text(output);
        wcscpy_s(output,MAX_PATH,pairs[i].directory);appended=PathAppendW(output,pairs[i].file);printf(",\"appendOk\":%u,\"appended\":",appended);text(output);putchar('}');
    }
}

int main(void) {
    static const LCID locales[]={0x409,0x41f,0x411,0x41d};
    LCID initial=GetThreadLocale();
    unsigned i;
    printf("{\"schema\":1,\"pointerBytes\":%u,\"ansi\":%u,\"rows\":[",(unsigned)sizeof(void *),GetACP());
    for (i=0;i<sizeof(locales)/sizeof(locales[0]);++i) {
        BOOL changed=SetThreadLocale(locales[i]);
        row("set-thread-locale");printf(",\"requested\":%lu,\"ok\":%u}",(unsigned long)locales[i],changed);
        comparison("umlaut-order",L"b",L"\x00e4");
        comparison("swedish-order",L"z",L"\x00e4");
        comparison("turkish-dotted",L"i",L"\x0130");
        comparison("turkish-dotless",L"\x0131",L"I");
    }
    SetThreadLocale(initial);
    prefixes(); paths();
    printf("]}\n");return 0;
}
