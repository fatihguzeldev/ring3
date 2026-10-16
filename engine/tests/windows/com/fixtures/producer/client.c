#include "contracts.h"
static void require(int condition,DWORD code){if(!condition)ExitProcess(code);}
static void register_server(const WCHAR *class,const WCHAR *path) {
    HKEY key=0;DWORD disposition=0;
    require(RegCreateKeyExW(HKEY_CURRENT_USER,class,0,0,0,KEY_ALL_ACCESS,0,&key,&disposition)==ERROR_SUCCESS,101);
    DWORD units=0;while(path[units])++units;
    require(RegSetValueExW(key,0,0,REG_SZ,(const BYTE *)path,(units+1)*2)==ERROR_SUCCESS,102);
    static const WCHAR threading[]=L"Both";
    require(RegSetValueExW(key,L"ThreadingModel",0,REG_SZ,(const BYTE *)threading,sizeof(threading))==ERROR_SUCCESS,103);
    require(RegCloseKey(key)==ERROR_SUCCESS,104);
}
static void check_object(OracleObject *object,DWORD code) {
    DWORD value=0;require(object!=0,code);
    require(object->table->value(object,&value)==S_OK&&value==0x13572468,code+1);
}
static HRESULT WINAPI outer_query(OracleFactory *self,const GUID *iid,void **out){(void)self;(void)iid;if(out)*out=0;return E_NOINTERFACE;}
static ULONG WINAPI outer_add_ref(OracleFactory *self){return ++self->refs;}
static ULONG WINAPI outer_release(OracleFactory *self){return --self->refs;}
static const OracleFactoryTable outer_table={outer_query,outer_add_ref,outer_release,0,0};
void entry(void) {
    OracleFactory outer={&outer_table,1};
    HANDLE log=CreateFileA("C:\\Temp\\com-activation.log",GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,0,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);
    require(log!=INVALID_HANDLE_VALUE,1);CloseHandle(log);
    OracleObject *object=(void *)1;OracleFactory *factory=0;void *alias=0;
    require(CoCreateInstance(&oracle_class,0,CLSCTX_INPROC_SERVER,&oracle_interface,(void **)&object)==CO_E_NOTINITIALIZED&&object==0,2);
    require(CoInitializeEx(0,COINIT_APARTMENTTHREADED)==S_OK,3);
    register_server(L"Software\\Classes\\CLSID\\{57383320-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",L"com-server.dll");
    register_server(L"Software\\Classes\\CLSID\\{57383320-9461-4d83-9146-2743d4e9ae02}\\InprocServer32",L"com-no-unload.dll");
    register_server(L"Software\\Classes\\CLSID\\{57383320-9461-4d83-9146-2743d4e9ae03}\\InprocServer32",L"com-failed.dll");
    register_server(L"Software\\Classes\\CLSID\\{57383320-9461-4d83-9146-2743d4e9ae04}\\InprocServer32",L"com-absent.dll");
    object=(void *)1;require(CoCreateInstance(&oracle_unknown_class,0,CLSCTX_INPROC_SERVER,&oracle_interface,(void **)&object)==REGDB_E_CLASSNOTREG&&object==0,4);
    require(CoGetClassObject(&oracle_class,CLSCTX_INPROC_SERVER,0,&oracle_factory_interface,(void **)&factory)==S_OK&&factory!=0,5);
    require(factory->table->query(factory,&unknown_iid,&alias)==S_OK&&alias==factory,6);
    require(factory->table->release(alias)==1,7);
    require(factory->table->lock(factory,TRUE)==S_OK,8);
    object=(void *)1;require(factory->table->create(factory,factory,&oracle_interface,(void **)&object)==CLASS_E_NOAGGREGATION&&object==0,9);
    object=(void *)1;require(factory->table->create(factory,0,&oracle_unknown_interface,(void **)&object)==E_NOINTERFACE&&object==0,10);
    require(factory->table->create(factory,0,&oracle_interface,(void **)&object)==S_OK,11);check_object(object,12);
    require(object->table->query(object,&unknown_iid,&alias)==S_OK&&alias==object,14);
    require(object->table->release(alias)==1,15);
    CoFreeUnusedLibrariesEx(0,0);require(GetModuleHandleA("com-server.dll")!=0,16);
    require(object->table->release(object)==0,17);
    require(factory->table->lock(factory,FALSE)==S_OK,18);
    require(factory->table->release(factory)==0,19);
    CoFreeUnusedLibrariesEx(0,0);require(GetModuleHandleA("com-server.dll")==0,20);
    object=(void *)1;require(CoCreateInstance(&oracle_class,(IUnknown *)&outer,CLSCTX_INPROC_SERVER,&oracle_interface,(void **)&object)==CLASS_E_NOAGGREGATION&&object==0,21);
    object=(void *)1;require(CoCreateInstance(&oracle_class,0,CLSCTX_INPROC_SERVER,&oracle_unknown_interface,(void **)&object)==E_NOINTERFACE&&object==0,22);
    require(CoCreateInstance(&oracle_class,0,CLSCTX_INPROC_SERVER,&oracle_interface,(void **)&object)==S_OK,23);check_object(object,24);
    require(object->table->release(object)==0,26);CoFreeUnusedLibrariesEx(0,0);require(GetModuleHandleA("com-server.dll")==0,27);
    factory=(void *)1;require(CoGetClassObject(&oracle_class,CLSCTX_INPROC_SERVER,0,&oracle_unknown_interface,(void **)&factory)==E_NOINTERFACE&&factory==0,28);
    CoFreeUnusedLibrariesEx(0,0);require(GetModuleHandleA("com-server.dll")==0,29);
    factory=(void *)1;require(CoGetClassObject(&oracle_missing_class,CLSCTX_INPROC_SERVER,0,&factory_iid,(void **)&factory)==CO_E_DLLNOTFOUND&&factory==0,30);
    factory=(void *)1;require(CoGetClassObject(&oracle_failed_class,CLSCTX_INPROC_SERVER,0,&factory_iid,(void **)&factory)==CO_E_ERRORINDLL&&factory==0,31);
    require(GetModuleHandleA("com-failed.dll")==0,32);
    require(CoCreateInstance(&oracle_no_unload_class,0,CLSCTX_INPROC_SERVER,&oracle_interface,(void **)&object)==S_OK,33);check_object(object,34);
    require(object->table->release(object)==0,36);CoFreeUnusedLibrariesEx(0,0);require(GetModuleHandleA("com-no-unload.dll")!=0,37);
    CoUninitialize();require(GetModuleHandleA("com-no-unload.dll")==0,38);
    static const char expected[]="AGqflcccirBrufUDAGcfGcfGcfrUDAGUDNGcfrn";
    char actual[sizeof(expected)];DWORD read=0;
    log=CreateFileA("C:\\Temp\\com-activation.log",GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,0,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,0);
    require(log!=INVALID_HANDLE_VALUE,39);require(ReadFile(log,actual,sizeof(actual),&read,0)&&read==sizeof(expected)-1,40);
    for(unsigned i=0;i<sizeof(expected)-1;i++)require(actual[i]==expected[i],41);
    CloseHandle(log);ExitProcess(0);
}
