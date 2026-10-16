#include "contracts.h"
static OracleObject objects[2];
static ULONG locks;
static HRESULT WINAPI object_query(OracleObject *self,const GUID *iid,void **out) {
    log_event('i'); if(!out)return E_POINTER; *out=0;
    if(!equal_guid(iid,&unknown_iid)&&!equal_guid(iid,&oracle_interface))return E_NOINTERFACE;
    ++self->refs; *out=self; return S_OK;
}
static ULONG WINAPI object_add_ref(OracleObject *self){return ++self->refs;}
static ULONG WINAPI object_release(OracleObject *self){log_event('r');return --self->refs;}
static HRESULT WINAPI object_value(OracleObject *self,DWORD *out){(void)self;if(!out)return E_POINTER;*out=0x13572468;return S_OK;}
static const OracleObjectTable object_table={object_query,object_add_ref,object_release,object_value};
static HRESULT WINAPI factory_query(OracleFactory *self,const GUID *iid,void **out) {
    log_event('q');if(!out)return E_POINTER;*out=0;
    if(!equal_guid(iid,&unknown_iid)&&!equal_guid(iid,&factory_iid)&&!equal_guid(iid,&oracle_factory_interface))return E_NOINTERFACE;
    ++self->refs;*out=self;return S_OK;
}
static ULONG WINAPI factory_add_ref(OracleFactory *self){return ++self->refs;}
static ULONG WINAPI factory_release(OracleFactory *self){log_event('f');return --self->refs;}
static HRESULT WINAPI factory_create(OracleFactory *self,void *outer,const GUID *iid,void **out) {
    (void)self;log_event('c');if(!out)return E_POINTER;*out=0;
    if(outer)return CLASS_E_NOAGGREGATION;
    if(!equal_guid(iid,&unknown_iid)&&!equal_guid(iid,&oracle_interface))return E_NOINTERFACE;
    for(unsigned i=0;i<2;i++)if(!objects[i].refs){objects[i].table=&object_table;objects[i].refs=1;*out=&objects[i];return S_OK;}
    return E_OUTOFMEMORY;
}
static HRESULT WINAPI factory_lock(OracleFactory *self,BOOL lock){(void)self;log_event(lock?'l':'u');if(lock)++locks;else if(locks)--locks;return S_OK;}
static const OracleFactoryTable factory_table={factory_query,factory_add_ref,factory_release,factory_create,factory_lock};
static OracleFactory factory={&factory_table,0};

HRESULT WINAPI DllGetClassObject(const GUID *class,const GUID *iid,void **out) {
    log_event('G');if(!out)return E_POINTER;*out=0;
#ifdef NO_CAN_UNLOAD
    const GUID *expected=&oracle_no_unload_class;
#else
    const GUID *expected=&oracle_class;
#endif
    if(!equal_guid(class,expected))return CLASS_E_CLASSNOTAVAILABLE;
    if(!equal_guid(iid,&unknown_iid)&&!equal_guid(iid,&factory_iid)&&!equal_guid(iid,&oracle_factory_interface))return E_NOINTERFACE;
    ++factory.refs;*out=&factory;return S_OK;
}
#ifndef NO_CAN_UNLOAD
HRESULT WINAPI DllCanUnloadNow(void) {
    const int busy=factory.refs||objects[0].refs||objects[1].refs||locks;
    log_event(busy?'B':'U');return busy?S_FALSE:S_OK;
}
#endif
BOOL WINAPI DllMain(HINSTANCE instance,DWORD reason,void *reserved) {
    (void)instance;(void)reserved;
#ifdef FAIL_ATTACH
    return reason!=DLL_PROCESS_ATTACH;
#else
    if(reason==DLL_PROCESS_ATTACH)log_event(
#ifdef NO_CAN_UNLOAD
        'N'
#else
        'A'
#endif
    );
    if(reason==DLL_PROCESS_DETACH)log_event(
#ifdef NO_CAN_UNLOAD
        'n'
#else
        'D'
#endif
    );
    return TRUE;
#endif
}
