#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shlwapi.h>
#include <stdio.h>

static int rows;
static void row(const char *name) {
    if (rows++) putchar(',');
    printf("{\"case\":\"%s\"", name);
}
static void comparison(const char *name, const WCHAR *left, const WCHAR *right) {
    row(name);
    printf(",\"userLocale\":%lu,\"threadLocale\":%lu,\"shell\":%d,\"shellPrefix1\":%d,\"userCompare\":%d,\"threadCompare\":%d}",
        (unsigned long)GetUserDefaultLCID(), (unsigned long)GetThreadLocale(),
        StrCmpIW(left,right), StrCmpNIW(left,right,1),
        CompareStringW(LOCALE_USER_DEFAULT,NORM_IGNORECASE,left,-1,right,-1)-CSTR_EQUAL,
        CompareStringW(GetThreadLocale(),NORM_IGNORECASE,left,-1,right,-1)-CSTR_EQUAL);
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
int main(void) {
    static const LCID locales[]={0x409,0x41f,0x411};
    LCID initial=GetThreadLocale();
    unsigned i;
    printf("{\"schema\":1,\"pointerBytes\":%u,\"ansi\":%u,\"rows\":[",(unsigned)sizeof(void *),GetACP());
    for (i=0;i<sizeof(locales)/sizeof(locales[0]);++i) {
        BOOL changed=SetThreadLocale(locales[i]);
        row("set-thread-locale");printf(",\"requested\":%lu,\"ok\":%u}",(unsigned long)locales[i],changed);
        comparison("umlaut-order",L"b",L"\x00e4");
        comparison("turkish-dotted",L"i",L"\x0130");
        comparison("turkish-dotless",L"\x0131",L"I");
    }
    SetThreadLocale(initial);
    prefixes();
    printf("]}\n");return 0;
}
