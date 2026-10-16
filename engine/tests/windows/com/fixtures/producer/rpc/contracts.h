#ifndef RING3_COM_RPC_ORACLE
#define RING3_COM_RPC_ORACLE
#define CINTERFACE
#define COBJMACROS
#include <windows.h>
#include <objbase.h>
#include <objidl.h>
#include <stddef.h>
_Static_assert(sizeof(void *)==4,"x86 guest");
_Static_assert(sizeof(RPCOLEMESSAGE)==44,"x86 RPC message");
_Static_assert(offsetof(RPCOLEMESSAGE,Buffer)==8&&offsetof(RPCOLEMESSAGE,cbBuffer)==12&&offsetof(RPCOLEMESSAGE,iMethod)==16&&offsetof(RPCOLEMESSAGE,rpcFlags)==40,"RPC message offsets");
_Static_assert(sizeof(IPSFactoryBufferVtbl)==20&&sizeof(IRpcProxyBufferVtbl)==20&&sizeof(IRpcStubBufferVtbl)==40&&sizeof(IRpcChannelBufferVtbl)==32,"COM proxy slots");
static const GUID source_class={0x57383601,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID proxy_class={0x57383602,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID value_iid={0x57383610,0x9461,0x4d83,{0x91,0x46,0x27,0x43,0xd4,0xe9,0xae,1}};
static const GUID unknown_iid={0,0,0,{0xc0,0,0,0,0,0,0,0x46}};
static const GUID factory_iid={1,0,0,{0xc0,0,0,0,0,0,0,0x46}};
static const GUID ps_iid={0xd5f569d0,0x593b,0x101a,{0xb5,0x69,8,0,0x2b,0x2d,0xbf,0x7a}};
static const GUID proxy_iid={0xd5f56a34,0x593b,0x101a,{0xb5,0x69,8,0,0x2b,0x2d,0xbf,0x7a}};
static const GUID stub_iid={0xd5f56afc,0x593b,0x101a,{0xb5,0x69,8,0,0x2b,0x2d,0xbf,0x7a}};
typedef struct Value Value;
typedef struct {
    HRESULT (WINAPI *query)(Value *,REFIID,void **);
    ULONG (WINAPI *add_ref)(Value *);
    ULONG (WINAPI *release)(Value *);
    HRESULT (WINAPI *get)(Value *,DWORD,DWORD *,DWORD *);
} ValueTable;
struct Value {const ValueTable *table;};
static void require(int condition,DWORD code) {if(!condition)ExitProcess(code);}
static int same(const GUID *a,const GUID *b) {
    if(a->Data1!=b->Data1||a->Data2!=b->Data2||a->Data3!=b->Data3)return 0;
    for(unsigned i=0;i<8;i++)if(a->Data4[i]!=b->Data4[i])return 0;
    return 1;
}
typedef struct {DWORD event,thread,role,value;} Record;
static void record(DWORD event,DWORD role,DWORD value) {
    Record row={event,GetCurrentThreadId(),role,value};DWORD done=0;
    HANDLE file=CreateFileA("C:\\Temp\\com-rpc.log",FILE_APPEND_DATA,FILE_SHARE_READ|FILE_SHARE_WRITE,0,OPEN_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);
    require(file!=INVALID_HANDLE_VALUE,220);require(WriteFile(file,&row,sizeof(row),&done,0)&&done==sizeof(row),221);require(CloseHandle(file),222);
}
#endif
