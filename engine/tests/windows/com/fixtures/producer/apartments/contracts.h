#ifndef RING3_COM_APARTMENT_ORACLE
#define RING3_COM_APARTMENT_ORACLE
#define CINTERFACE
#include <windows.h>
#include <objbase.h>

static const GUID classes[4] = {
    {0x57383401,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}},
    {0x57383402,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}},
    {0x57383403,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}},
    {0x57383404,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}},
};
static const GUID unknown_iid = {0,0,0,{0xc0,0,0,0,0,0,0,0x46}};
static const GUID factory_iid = {1,0,0,{0xc0,0,0,0,0,0,0,0x46}};
typedef struct OracleUnknown OracleUnknown;
typedef struct OracleFactory OracleFactory;
typedef struct {
    HRESULT (WINAPI *query)(OracleUnknown *,const GUID *,void **);
    ULONG (WINAPI *add_ref)(OracleUnknown *);
    ULONG (WINAPI *release)(OracleUnknown *);
} UnknownTable;
struct OracleUnknown {const UnknownTable *table; ULONG refs; DWORD owner; BOOL orphan;};
typedef struct {
    HRESULT (WINAPI *query)(OracleFactory *,const GUID *,void **);
    ULONG (WINAPI *add_ref)(OracleFactory *);
    ULONG (WINAPI *release)(OracleFactory *);
    HRESULT (WINAPI *create)(OracleFactory *,void *,const GUID *,void **);
    HRESULT (WINAPI *lock)(OracleFactory *,BOOL);
} FactoryTable;
struct OracleFactory {const FactoryTable *table; ULONG refs;};
typedef struct {DWORD event,thread,model,value;} Record;
static void require(int condition,DWORD code) {if(!condition)ExitProcess(code);}
static int equal_guid(const GUID *a,const GUID *b) {
    if(a->Data1!=b->Data1||a->Data2!=b->Data2||a->Data3!=b->Data3)return 0;
    for(unsigned i=0;i<8;i++)if(a->Data4[i]!=b->Data4[i])return 0;
    return 1;
}
static void record(DWORD event,DWORD model,DWORD value) {
    Record row={event,GetCurrentThreadId(),model,value};DWORD written=0;
    HANDLE log=CreateFileA("C:\\Temp\\com-apartments.log",FILE_APPEND_DATA,FILE_SHARE_READ|FILE_SHARE_WRITE,0,OPEN_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);
    require(log!=INVALID_HANDLE_VALUE,200);
    require(WriteFile(log,&row,sizeof(row),&written,0)&&written==sizeof(row),201);
    require(CloseHandle(log),202);
}
#endif
