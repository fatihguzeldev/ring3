#include "contracts.h"
static OracleUnknown objects[8];
static ULONG locks;
static DWORD owner;
static void check_apartment(void) {
    DWORD tid=GetCurrentThreadId();
#if MODEL == 1 || MODEL == 3
    require(!owner||owner==tid,210+MODEL);
#endif
#if MODEL == 4
    require(CoInitializeEx(0,COINIT_MULTITHREADED)==RPC_E_CHANGED_MODE,220);
#endif
}
static HRESULT WINAPI object_query(OracleUnknown *self,const GUID *iid,void **out) {
    check_apartment();record('Q',MODEL,(DWORD)self);
    if(!out)return E_POINTER;*out=0;
    if(!equal_guid(iid,&unknown_iid))return E_NOINTERFACE;
    ++self->refs;*out=self;return S_OK;
}
static ULONG WINAPI object_add_ref(OracleUnknown *self){check_apartment();return ++self->refs;}
static ULONG WINAPI object_release(OracleUnknown *self) {
    check_apartment();record('R',MODEL,(DWORD)self);
    ULONG refs=--self->refs;
    if(!refs&&self->orphan) {
        HANDLE released=OpenEventA(EVENT_MODIFY_STATE,FALSE,"com-apartment-released");
        require(released!=0,221);require(SetEvent(released),222);CloseHandle(released);
    }
    return refs;
}
static const UnknownTable object_table={object_query,object_add_ref,object_release};
static HRESULT WINAPI factory_query(OracleFactory *self,const GUID *iid,void **out) {
    check_apartment();record('q',MODEL,(DWORD)self);
    if(!out)return E_POINTER;*out=0;
    if(!equal_guid(iid,&unknown_iid)&&!equal_guid(iid,&factory_iid))return E_NOINTERFACE;
    ++self->refs;*out=self;return S_OK;
}
static ULONG WINAPI factory_add_ref(OracleFactory *self){check_apartment();return ++self->refs;}
static ULONG WINAPI factory_release(OracleFactory *self){check_apartment();record('F',MODEL,(DWORD)self);return --self->refs;}
static HRESULT WINAPI factory_create(OracleFactory *self,void *outer,const GUID *iid,void **out) {
    (void)self;check_apartment();record('C',MODEL,0);
    if(!out)return E_POINTER;*out=0;if(outer)return CLASS_E_NOAGGREGATION;
    if(!equal_guid(iid,&unknown_iid))return E_NOINTERFACE;
    BOOL orphan=FALSE;
#if MODEL == 1
    HANDLE go=OpenEventA(SYNCHRONIZE,FALSE,"com-apartment-go");
    if(go) {
        HANDLE entered=OpenEventA(EVENT_MODIFY_STATE,FALSE,"com-apartment-entered");
        require(entered!=0,223);record('B',MODEL,0);require(SetEvent(entered),224);CloseHandle(entered);
        require(WaitForSingleObject(go,INFINITE)==WAIT_OBJECT_0,225);CloseHandle(go);orphan=TRUE;
    }
#endif
#if MODEL == 2
    {
        OracleFactory *nested=0;
        record('N',MODEL,0);
        require(CoGetClassObject(&classes[2],CLSCTX_INPROC_SERVER,0,&factory_iid,(void **)&nested)==S_OK&&nested!=0,226);
        require(nested->table->lock(nested,TRUE)==S_OK,227);
        require(nested->table->lock(nested,FALSE)==S_OK,228);
        nested->table->release(nested);record('n',MODEL,0);
    }
#endif
    for(unsigned i=0;i<8;i++)if(!objects[i].refs) {
        objects[i].table=&object_table;objects[i].refs=1;objects[i].owner=GetCurrentThreadId();objects[i].orphan=orphan;
        *out=&objects[i];record('O',MODEL,(DWORD)*out);return S_OK;
    }
    return E_OUTOFMEMORY;
}
static HRESULT WINAPI factory_lock(OracleFactory *self,BOOL locked) {
    (void)self;check_apartment();record(locked?'L':'l',MODEL,0);
    if(locked)++locks;else if(locks)--locks;return S_OK;
}
static const FactoryTable factory_table={factory_query,factory_add_ref,factory_release,factory_create,factory_lock};
static OracleFactory factory={&factory_table,0};
HRESULT WINAPI DllGetClassObject(const GUID *class,const GUID *iid,void **out) {
    check_apartment();owner=GetCurrentThreadId();record('G',MODEL,(DWORD)&factory);
    if(!out)return E_POINTER;*out=0;
    if(!equal_guid(class,&classes[MODEL-1]))return CLASS_E_CLASSNOTAVAILABLE;
    if(!equal_guid(iid,&unknown_iid)&&!equal_guid(iid,&factory_iid))return E_NOINTERFACE;
    ++factory.refs;*out=&factory;return S_OK;
}
HRESULT WINAPI DllCanUnloadNow(void) {
    BOOL busy=factory.refs||locks;
    for(unsigned i=0;i<8;i++)busy|=objects[i].refs!=0;
    record(busy?'b':'u',MODEL,0);return busy?S_FALSE:S_OK;
}
BOOL WINAPI DllMain(HINSTANCE instance,DWORD reason,void *reserved) {
    (void)instance;(void)reserved;
    if(reason==DLL_PROCESS_ATTACH)record('A',MODEL,0);
    if(reason==DLL_PROCESS_DETACH)record('D',MODEL,0);
    return TRUE;
}
