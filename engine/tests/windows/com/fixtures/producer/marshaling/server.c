#include "contracts.h"
#ifndef ROLE
#define ROLE 0
#endif
typedef struct {const IMarshalVtbl *table;Value *value;} Marshal;
struct Value {const ValueTable *table;ULONG refs;DWORD owner,value;BOOL copy;Marshal marshal;};
static Value pool[16];
static ULONG live;
static const ValueTable value_table;
static const IMarshalVtbl marshal_table;
static Value *allocate(DWORD value,BOOL copy) {
    for(unsigned i=0;i<16;i++)if(!pool[i].refs){
        Value *object=&pool[i];object->table=&value_table;object->refs=1;object->owner=GetCurrentThreadId();object->value=value;object->copy=copy;object->marshal.table=&marshal_table;object->marshal.value=object;++live;return object;
    }
    return 0;
}
static ULONG WINAPI add(Value *object) {return ++object->refs;}
static ULONG WINAPI release(Value *object) {
    require(object->refs!=0,223);ULONG refs=--object->refs;record('r',ROLE,refs);if(!refs)--live;return refs;
}
static HRESULT WINAPI query(Value *object,const GUID *iid,void **out) {
    if(!out)return E_POINTER;*out=0;
    if(same(iid,&unknown_iid)||same(iid,&value_iid)||same(iid,&fail_iid))*out=object;
    else if(same(iid,&marshal_iid))*out=&object->marshal;
    else return E_NOINTERFACE;
    add(object);return S_OK;
}
static HRESULT WINAPI get(Value *object,DWORD *value,DWORD *owner) {
    if(!value||!owner)return E_POINTER;*value=object->value;*owner=object->owner;record('v',ROLE,*value);return S_OK;
}
static const ValueTable value_table={query,add,release,get};
static Value *object(IMarshal *self) {return ((Marshal *)self)->value;}
static HRESULT WINAPI marshal_query(IMarshal *self,REFIID iid,void **out) {return query(object(self),iid,out);}
static ULONG WINAPI marshal_add(IMarshal *self) {return add(object(self));}
static ULONG WINAPI marshal_release(IMarshal *self) {return release(object(self));}
static HRESULT WINAPI marshal_class(IMarshal *self,REFIID iid,void *pointer,DWORD context,void *reserved,DWORD flags,CLSID *out) {
    require(!ROLE&&GetCurrentThreadId()==object(self)->owner,224);
    require((same(iid,&value_iid)||same(iid,&fail_iid))&&pointer==object(self)&&context==MSHCTX_INPROC&&!reserved&&flags==MSHLFLAGS_NORMAL&&out,225);
    *out=copy_class;record('g',ROLE,context);return S_OK;
}
static HRESULT WINAPI marshal_size(IMarshal *self,REFIID iid,void *pointer,DWORD context,void *reserved,DWORD flags,DWORD *out) {
    require(!ROLE&&GetCurrentThreadId()==object(self)->owner,226);
    require((same(iid,&value_iid)||same(iid,&fail_iid))&&pointer==object(self)&&context==MSHCTX_INPROC&&!reserved&&flags==MSHLFLAGS_NORMAL&&out,227);
    *out=8;record('s',ROLE,*out);return S_OK;
}
static HRESULT WINAPI marshal_write(IMarshal *self,IStream *stream,REFIID iid,void *pointer,DWORD context,void *reserved,DWORD flags) {
    require(!ROLE&&GetCurrentThreadId()==object(self)->owner,228);
    require((same(iid,&value_iid)||same(iid,&fail_iid))&&pointer==object(self)&&context==MSHCTX_INPROC&&!reserved&&flags==MSHLFLAGS_NORMAL&&stream,229);
    DWORD packet[2]={0x314d4352,object(self)->value},done=0;IStream *clone=0;STATSTG stat;
    require(IStream_Stat(stream,&stat,STATFLAG_NONAME)==S_OK&&stat.type==STGTY_STREAM&&stat.cbSize.QuadPart==0,230);
    require(IStream_Write(stream,packet,8,&done)==S_OK&&done==8,231);
    require(IStream_Clone(stream,&clone)==S_OK&&clone,232);
    LARGE_INTEGER zero;zero.QuadPart=0;ULARGE_INTEGER at;
    require(IStream_Seek(clone,zero,STREAM_SEEK_SET,&at)==S_OK&&at.QuadPart==0,233);
    DWORD copy[2]={0,0};done=0;require(IStream_Read(clone,copy,8,&done)==S_OK&&done==8&&copy[0]==packet[0]&&copy[1]==packet[1],234);
    IStream_Release(clone);
    require(IStream_Seek(stream,zero,STREAM_SEEK_CUR,&at)==S_OK&&at.QuadPart==8,235);
    require(IStream_Stat(stream,&stat,STATFLAG_NONAME)==S_OK&&stat.cbSize.QuadPart==8,236);
    require(IStream_Commit(stream,STGC_DEFAULT)==S_OK,237);record('w',ROLE,packet[1]);return S_OK;
}
static HRESULT WINAPI unmarshal(IMarshal *self,IStream *stream,REFIID iid,void **out) {
    require(ROLE&&GetCurrentThreadId()==object(self)->owner,238);if(!out)return E_POINTER;*out=0;
    DWORD packet[2]={0,0},done=0;require(IStream_Read(stream,packet,8,&done)==S_OK&&done==8&&packet[0]==0x314d4352,239);record('u',ROLE,packet[1]);
    if(same(iid,&fail_iid))return E_FAIL;
    if(!same(iid,&value_iid))return E_NOINTERFACE;
    Value *copy=allocate(packet[1],TRUE);if(!copy)return E_OUTOFMEMORY;*out=copy;return S_OK;
}
static HRESULT WINAPI release_packet(IMarshal *self,IStream *stream) {
    require(!ROLE&&GetCurrentThreadId()==object(self)->owner,240);
    DWORD packet[2]={0,0},done=0;require(IStream_Read(stream,packet,8,&done)==S_OK&&done==8&&packet[0]==0x314d4352,241);record('d',ROLE,packet[1]);return S_OK;
}
static HRESULT WINAPI disconnect(IMarshal *self,DWORD reserved) {(void)self;return reserved?E_INVALIDARG:S_OK;}
static const IMarshalVtbl marshal_table={marshal_query,marshal_add,marshal_release,marshal_class,marshal_size,marshal_write,unmarshal,release_packet,disconnect};
typedef struct {const IClassFactoryVtbl *table;ULONG refs;} Factory;
static ULONG WINAPI factory_add(IClassFactory *self) {return ++((Factory *)self)->refs;}
static ULONG WINAPI factory_release(IClassFactory *self) {return --((Factory *)self)->refs;}
static HRESULT WINAPI factory_query(IClassFactory *self,REFIID iid,void **out) {
    if(!out)return E_POINTER;*out=0;if(!same(iid,&unknown_iid)&&!same(iid,&factory_iid))return E_NOINTERFACE;*out=self;factory_add(self);return S_OK;
}
static HRESULT WINAPI create(IClassFactory *self,IUnknown *outer,REFIID iid,void **out) {
    (void)self;if(!out)return E_POINTER;*out=0;if(outer)return CLASS_E_NOAGGREGATION;
    Value *value=allocate(ROLE?0:0x13579bdf,ROLE);if(!value)return E_OUTOFMEMORY;
    record('c',ROLE,value->owner);HRESULT result=query(value,iid,out);release(value);return result;
}
static HRESULT WINAPI lock(IClassFactory *self,BOOL value) {(void)self;(void)value;return S_OK;}
static const IClassFactoryVtbl factory_table={factory_query,factory_add,factory_release,create,lock};
static Factory factory={&factory_table,0};
HRESULT WINAPI DllGetClassObject(REFCLSID clsid,REFIID iid,void **out) {
    if(!out)return E_POINTER;*out=0;if(!same(clsid,ROLE?&copy_class:&source_class))return CLASS_E_CLASSNOTAVAILABLE;
    record('G',ROLE,0);return factory_query((IClassFactory *)&factory,iid,out);
}
HRESULT WINAPI DllCanUnloadNow(void) {record('U',ROLE,live);return live||factory.refs?S_FALSE:S_OK;}
BOOL WINAPI DllMain(HINSTANCE image,DWORD reason,void *reserved) {(void)image;(void)reserved;if(reason==DLL_PROCESS_ATTACH)record('A',ROLE,0);if(reason==DLL_PROCESS_DETACH)record('D',ROLE,0);return TRUE;}
