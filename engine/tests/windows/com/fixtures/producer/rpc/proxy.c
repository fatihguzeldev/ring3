#include "contracts.h"
typedef struct {Value value;IRpcProxyBuffer control;ULONG refs;DWORD owner;IUnknown *outer;IRpcChannelBuffer *channel;} Proxy;
typedef struct {IRpcStubBuffer control;ULONG refs;DWORD owner;Value *server;} Stub;
static Proxy proxies[16];static Stub stubs[16];static ULONG live,factory_refs;
static Proxy *proxy(IRpcProxyBuffer *self) {return (Proxy *)((BYTE *)self - offsetof(Proxy,control));}
static ULONG WINAPI proxy_add(IRpcProxyBuffer *self) {return ++proxy(self)->refs;}
static void WINAPI proxy_disconnect(IRpcProxyBuffer *self) {Proxy *p=proxy(self);require(GetCurrentThreadId()==p->owner,240);if(p->channel){IRpcChannelBuffer_Release(p->channel);p->channel=0;record('d',1,0);}}
static ULONG WINAPI proxy_release(IRpcProxyBuffer *self) {Proxy *p=proxy(self);require(p->refs&&GetCurrentThreadId()==p->owner,241);ULONG refs=--p->refs;if(!refs){proxy_disconnect(self);p->outer=0;--live;}record('r',1,refs);return refs;}
static HRESULT WINAPI proxy_query(IRpcProxyBuffer *self,REFIID iid,void **out) {if(!out)return E_POINTER;*out=0;if(!same(iid,&unknown_iid)&&!same(iid,&proxy_iid))return E_NOINTERFACE;*out=self;proxy_add(self);return S_OK;}
static HRESULT WINAPI proxy_connect(IRpcProxyBuffer *self,IRpcChannelBuffer *channel) {Proxy *p=proxy(self);require(GetCurrentThreadId()==p->owner,242);if(!channel)return E_POINTER;if(p->channel)return E_UNEXPECTED;DWORD context=0;void *reserved=(void *)1;require(IRpcChannelBuffer_GetDestCtx(channel,&context,&reserved)==S_OK&&context==MSHCTX_INPROC&&!reserved,243);require(IRpcChannelBuffer_IsConnected(channel)==S_OK,244);IRpcChannelBuffer_AddRef(channel);p->channel=channel;record('C',1,context);return S_OK;}
static const IRpcProxyBufferVtbl proxy_table={proxy_query,proxy_add,proxy_release,proxy_connect,proxy_disconnect};
static HRESULT WINAPI value_query(Value *self,REFIID iid,void **out) {return IUnknown_QueryInterface(((Proxy *)self)->outer,iid,out);}
static ULONG WINAPI value_add(Value *self) {return IUnknown_AddRef(((Proxy *)self)->outer);}
static ULONG WINAPI value_release(Value *self) {return IUnknown_Release(((Proxy *)self)->outer);}
static HRESULT WINAPI value_get(Value *self,DWORD selector,DWORD *value,DWORD *owner) {
    Proxy *p=(Proxy *)self;require(GetCurrentThreadId()==p->owner,245);if(!p->channel)return RPC_E_DISCONNECTED;if(!value||!owner)return E_POINTER;
    RPCOLEMESSAGE message={0};message.dataRepresentation=0x10;message.cbBuffer=4;message.iMethod=3;message.rpcFlags=0;
    require(IRpcChannelBuffer_GetBuffer(p->channel,&message,&value_iid)==S_OK&&message.Buffer,246);*(DWORD *)message.Buffer=selector;ULONG status=0xffffffff;
    HRESULT result=IRpcChannelBuffer_SendReceive(p->channel,&message,&status);if(FAILED(result)){IRpcChannelBuffer_FreeBuffer(p->channel,&message);return result;}
    require(status==0&&message.cbBuffer==12&&message.Buffer&&message.dataRepresentation==0x10&&message.iMethod==3,247);DWORD *packet=message.Buffer;result=(HRESULT)packet[0];*value=packet[1];*owner=packet[2];record('p',1,selector);
    require(IRpcChannelBuffer_FreeBuffer(p->channel,&message)==S_OK,248);return result;
}
static const ValueTable value_table={value_query,value_add,value_release,value_get};
static ULONG WINAPI stub_add(IRpcStubBuffer *self) {return ++((Stub *)self)->refs;}
static void WINAPI stub_disconnect(IRpcStubBuffer *self) {Stub *s=(Stub *)self;require(GetCurrentThreadId()==s->owner,249);if(s->server){s->server->table->release(s->server);s->server=0;record('d',2,0);}}
static ULONG WINAPI stub_release(IRpcStubBuffer *self) {Stub *s=(Stub *)self;require(s->refs&&GetCurrentThreadId()==s->owner,250);ULONG refs=--s->refs;if(!refs){stub_disconnect(self);--live;}record('r',2,refs);return refs;}
static HRESULT WINAPI stub_query(IRpcStubBuffer *self,REFIID iid,void **out) {if(!out)return E_POINTER;*out=0;if(!same(iid,&unknown_iid)&&!same(iid,&stub_iid))return E_NOINTERFACE;*out=self;stub_add(self);return S_OK;}
static HRESULT WINAPI stub_connect(IRpcStubBuffer *self,IUnknown *server) {Stub *s=(Stub *)self;if(!server)return E_POINTER;if(s->server)return E_UNEXPECTED;return IUnknown_QueryInterface(server,&value_iid,(void **)&s->server);}
static HRESULT WINAPI stub_invoke(IRpcStubBuffer *self,RPCOLEMESSAGE *message,IRpcChannelBuffer *channel) {
    Stub *s=(Stub *)self;require(GetCurrentThreadId()==s->owner&&s->server,251);if(!message||!channel)return E_POINTER;require(message->iMethod==3&&message->cbBuffer==4&&message->Buffer&&message->dataRepresentation==0x10,252);
    DWORD selector=*(DWORD *)message->Buffer,value=0,owner=0;HRESULT result=s->server->table->get(s->server,selector,&value,&owner);message->cbBuffer=12;
    require(IRpcChannelBuffer_GetBuffer(channel,message,&value_iid)==S_OK&&message->Buffer,253);DWORD *packet=message->Buffer;packet[0]=(DWORD)result;packet[1]=value;packet[2]=owner;record('i',2,selector);return S_OK;
}
static IRpcStubBuffer *WINAPI stub_supported(IRpcStubBuffer *self,REFIID iid) {return same(iid,&value_iid)?self:0;}
static ULONG WINAPI stub_count(IRpcStubBuffer *self) {return ((Stub *)self)->refs;}
static HRESULT WINAPI stub_debug_query(IRpcStubBuffer *self,void **out) {if(!out)return E_POINTER;*out=((Stub *)self)->server;return S_OK;}
static void WINAPI stub_debug_release(IRpcStubBuffer *self,void *pointer) {(void)self;(void)pointer;}
static const IRpcStubBufferVtbl stub_table={stub_query,stub_add,stub_release,stub_connect,stub_disconnect,stub_invoke,stub_supported,stub_count,stub_debug_query,stub_debug_release};
static ULONG WINAPI factory_add(IPSFactoryBuffer *self) {(void)self;return ++factory_refs;}
static ULONG WINAPI factory_release(IPSFactoryBuffer *self) {(void)self;require(factory_refs!=0,254);return --factory_refs;}
static HRESULT WINAPI factory_query(IPSFactoryBuffer *self,REFIID iid,void **out) {if(!out)return E_POINTER;*out=0;if(!same(iid,&unknown_iid)&&!same(iid,&ps_iid))return E_NOINTERFACE;*out=self;factory_add(self);return S_OK;}
static HRESULT WINAPI create_proxy(IPSFactoryBuffer *self,IUnknown *outer,REFIID iid,IRpcProxyBuffer **control,void **out) {
    (void)self;if(!control||!out)return E_POINTER;*control=0;*out=0;if(!outer)return E_INVALIDARG;if(!same(iid,&value_iid))return E_NOINTERFACE;
    for(unsigned i=0;i<16;i++)if(!proxies[i].refs){Proxy *p=&proxies[i];p->value.table=&value_table;p->control.lpVtbl=&proxy_table;p->refs=1;p->owner=GetCurrentThreadId();p->outer=outer;p->channel=0;*control=&p->control;*out=&p->value;IUnknown_AddRef(outer);++live;record('P',1,p->owner);return S_OK;}return E_OUTOFMEMORY;
}
static HRESULT WINAPI create_stub(IPSFactoryBuffer *self,REFIID iid,IUnknown *server,IRpcStubBuffer **out) {
    (void)self;if(!out)return E_POINTER;*out=0;if(!same(iid,&value_iid))return E_NOINTERFACE;for(unsigned i=0;i<16;i++)if(!stubs[i].refs){Stub *s=&stubs[i];s->control.lpVtbl=&stub_table;s->refs=1;s->owner=GetCurrentThreadId();s->server=0;HRESULT result=stub_connect(&s->control,server);if(FAILED(result)){s->refs=0;return result;}*out=&s->control;++live;record('S',2,s->owner);return S_OK;}return E_OUTOFMEMORY;
}
static const IPSFactoryBufferVtbl factory_table={factory_query,factory_add,factory_release,create_proxy,create_stub};static IPSFactoryBuffer factory={&factory_table};
HRESULT WINAPI DllGetClassObject(REFCLSID clsid,REFIID iid,void **out) {if(!out)return E_POINTER;*out=0;if(!same(clsid,&proxy_class))return CLASS_E_CLASSNOTAVAILABLE;record('G',1,GetCurrentThreadId());return factory_query(&factory,iid,out);}
HRESULT WINAPI DllCanUnloadNow(void) {record('U',1,live+factory_refs);return live||factory_refs?S_FALSE:S_OK;}
BOOL WINAPI DllMain(HINSTANCE image,DWORD reason,void *reserved) {(void)image;(void)reserved;if(reason==DLL_PROCESS_ATTACH)record('A',1,0);if(reason==DLL_PROCESS_DETACH)record('D',1,0);return TRUE;}
