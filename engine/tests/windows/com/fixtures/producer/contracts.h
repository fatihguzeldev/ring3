#ifndef RING3_COM_ORACLE_CONTRACTS
#define RING3_COM_ORACLE_CONTRACTS
#define CINTERFACE
#include <windows.h>
#include <objbase.h>

static const GUID oracle_class = {0x57383320,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID oracle_no_unload_class = {0x57383320,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,2}};
static const GUID oracle_failed_class = {0x57383320,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,3}};
static const GUID oracle_missing_class = {0x57383320,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,4}};
static const GUID oracle_unknown_class = {0x57383320,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,5}};
static const GUID oracle_interface = {0x57383321,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID oracle_factory_interface = {0x57383322,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID oracle_unknown_interface = {0x57383323,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID unknown_iid = {0,0,0,{0xc0,0,0,0,0,0,0,0x46}};
static const GUID factory_iid = {1,0,0,{0xc0,0,0,0,0,0,0,0x46}};

typedef struct OracleFactory OracleFactory;
typedef struct OracleObject OracleObject;
typedef struct {
    HRESULT (WINAPI *query)(OracleFactory *,const GUID *,void **);
    ULONG (WINAPI *add_ref)(OracleFactory *);
    ULONG (WINAPI *release)(OracleFactory *);
    HRESULT (WINAPI *create)(OracleFactory *,void *,const GUID *,void **);
    HRESULT (WINAPI *lock)(OracleFactory *,BOOL);
} OracleFactoryTable;
struct OracleFactory {const OracleFactoryTable *table; ULONG refs;};
typedef struct {
    HRESULT (WINAPI *query)(OracleObject *,const GUID *,void **);
    ULONG (WINAPI *add_ref)(OracleObject *);
    ULONG (WINAPI *release)(OracleObject *);
    HRESULT (WINAPI *value)(OracleObject *,DWORD *);
} OracleObjectTable;
struct OracleObject {const OracleObjectTable *table; ULONG refs;};

static int equal_guid(const GUID *a,const GUID *b) {
    if(a->Data1!=b->Data1||a->Data2!=b->Data2||a->Data3!=b->Data3)return 0;
    for(unsigned i=0;i<8;i++)if(a->Data4[i]!=b->Data4[i])return 0;
    return 1;
}
static void log_event(char event) {
    HANDLE file=CreateFileA("C:\\Temp\\com-activation.log",FILE_APPEND_DATA,FILE_SHARE_READ|FILE_SHARE_WRITE,0,OPEN_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);
    DWORD written=0;
    if(file==INVALID_HANDLE_VALUE||!WriteFile(file,&event,1,&written,0)||written!=1)ExitProcess(200);
    CloseHandle(file);
}
#endif
