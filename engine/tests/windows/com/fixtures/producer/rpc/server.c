#include "contracts.h"
typedef struct {Value value;ULONG refs;DWORD owner;} Object;
static Object pool[16];static ULONG live,factory_refs;
static ULONG WINAPI add(Value *self) {return ++((Object *)self)->refs;}
static ULONG WINAPI release(Value *self) {Object *object=(Object *)self;require(object->refs&&GetCurrentThreadId()==object->owner,230);ULONG refs=--object->refs;if(!refs)--live;record('r',0,refs);return refs;}
static HRESULT WINAPI query(Value *self,REFIID iid,void **out) {
    if(!out)return E_POINTER;*out=0;if(!same(iid,&unknown_iid)&&!same(iid,&value_iid))return E_NOINTERFACE;*out=self;add(self);return S_OK;
}
static HRESULT WINAPI get(Value *self,DWORD selector,DWORD *value,DWORD *owner) {
    Object *object=(Object *)self;require(GetCurrentThreadId()==object->owner,231);if(!value||!owner)return E_POINTER;
    *owner=object->owner;*value=0;record('v',0,selector);if(selector)return E_FAIL;*value=0x2468ace0;return S_OK;
}
static const ValueTable value_table={query,add,release,get};
static ULONG WINAPI factory_add(IClassFactory *self) {(void)self;return ++factory_refs;}
static ULONG WINAPI factory_release(IClassFactory *self) {(void)self;require(factory_refs!=0,232);return --factory_refs;}
static HRESULT WINAPI factory_query(IClassFactory *self,REFIID iid,void **out) {if(!out)return E_POINTER;*out=0;if(!same(iid,&unknown_iid)&&!same(iid,&factory_iid))return E_NOINTERFACE;*out=self;factory_add(self);return S_OK;}
static HRESULT WINAPI create(IClassFactory *self,IUnknown *outer,REFIID iid,void **out) {
    (void)self;if(!out)return E_POINTER;*out=0;if(outer)return CLASS_E_NOAGGREGATION;
    for(unsigned i=0;i<16;i++)if(!pool[i].refs){Object *object=&pool[i];object->value.table=&value_table;object->refs=1;object->owner=GetCurrentThreadId();++live;record('c',0,object->owner);HRESULT result=query(&object->value,iid,out);release(&object->value);return result;}return E_OUTOFMEMORY;
}
static HRESULT WINAPI lock(IClassFactory *self,BOOL value) {(void)self;(void)value;return S_OK;}
static const IClassFactoryVtbl table={factory_query,factory_add,factory_release,create,lock};static IClassFactory factory={&table};
HRESULT WINAPI DllGetClassObject(REFCLSID clsid,REFIID iid,void **out) {if(!out)return E_POINTER;*out=0;if(!same(clsid,&source_class))return CLASS_E_CLASSNOTAVAILABLE;record('G',0,0);return factory_query(&factory,iid,out);}
HRESULT WINAPI DllCanUnloadNow(void) {record('U',0,live+factory_refs);return live||factory_refs?S_FALSE:S_OK;}
BOOL WINAPI DllMain(HINSTANCE image,DWORD reason,void *reserved) {(void)image;(void)reserved;if(reason==DLL_PROCESS_ATTACH)record('A',0,0);if(reason==DLL_PROCESS_DETACH)record('D',0,0);return TRUE;}
