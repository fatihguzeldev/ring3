#include "contracts.h"
static DWORD main_thread;
static void register_server(unsigned model,const WCHAR *path,const WCHAR *threading) {
    static const WCHAR *keys[4]={
        L"Software\\Classes\\CLSID\\{57383401-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",
        L"Software\\Classes\\CLSID\\{57383402-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",
        L"Software\\Classes\\CLSID\\{57383403-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",
        L"Software\\Classes\\CLSID\\{57383404-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",
    };
    HKEY key=0;DWORD disposition=0,units=0;
    require(RegCreateKeyExW(HKEY_CURRENT_USER,keys[model-1],0,0,0,KEY_ALL_ACCESS,0,&key,&disposition)==ERROR_SUCCESS,101);
    while(path[units])++units;
    require(RegSetValueExW(key,0,0,REG_SZ,(const BYTE *)path,(units+1)*2)==ERROR_SUCCESS,102);
    if(threading) {
        units=0;while(threading[units])++units;
        require(RegSetValueExW(key,L"ThreadingModel",0,REG_SZ,(const BYTE *)threading,(units+1)*2)==ERROR_SUCCESS,103);
    }
    require(RegCloseKey(key)==ERROR_SUCCESS,104);
}
static void use_factory(unsigned model) {
    OracleFactory *factory=0,*factory_again=0;OracleUnknown *identity=0,*identity_again=0,*object=0;
    require(CoGetClassObject(&classes[model-1],CLSCTX_INPROC_SERVER,0,&factory_iid,(void **)&factory)==S_OK&&factory,10+model);
    record('p',model,(DWORD)factory);
    require(factory->table->query(factory,&unknown_iid,(void **)&identity)==S_OK&&identity,20+model);
    require(identity->table->query(identity,&unknown_iid,(void **)&identity_again)==S_OK&&identity_again==identity,30+model);
    require(identity->table->query(identity,&factory_iid,(void **)&factory_again)==S_OK&&factory_again==factory,40+model);
    factory_again->table->release(factory_again);identity_again->table->release(identity_again);identity->table->release(identity);
    require(factory->table->lock(factory,TRUE)==S_OK,50+model);
    require(factory->table->lock(factory,FALSE)==S_OK,60+model);
    require(factory->table->create(factory,0,&unknown_iid,(void **)&object)==S_OK&&object,70+model);
    record('o',model,(DWORD)object);
    require(object->table->query(object,&factory_iid,(void **)&factory_again)==E_NOINTERFACE&&factory_again==0,80+model);
    require(object->table->release(object)==0,90+model);
    require(factory->table->release(factory)==0,110+model);
}
static DWORD WINAPI worker(void *argument) {
    (void)argument;require(GetCurrentThreadId()!=main_thread,120);
    require(CoInitializeEx(0,COINIT_MULTITHREADED)==S_OK,121);record('w',1,main_thread);
    use_factory(1);CoUninitialize();record('W',1,0);return 0;
}
static DWORD WINAPI orphan_worker(void *argument) {
    (void)argument;OracleUnknown *object=(void *)1;
    require(CoInitializeEx(0,COINIT_MULTITHREADED)==S_OK,122);record('w',1,main_thread);
    HRESULT result=CoCreateInstance(&classes[0],0,CLSCTX_INPROC_SERVER,&unknown_iid,(void **)&object);
    record('!',1,(DWORD)result);ExitProcess(123);return 1;
}
static void orphan_case(void) {
    HANDLE go=CreateEventA(0,TRUE,FALSE,"com-apartment-go");
    HANDLE entered=CreateEventA(0,TRUE,FALSE,"com-apartment-entered");
    HANDLE released=CreateEventA(0,TRUE,FALSE,"com-apartment-released");
    require(go&&entered&&released,124);
    DWORD tid=0;HANDLE thread=CreateThread(0,0,orphan_worker,0,0,&tid);require(thread!=0,125);
    require(WaitForSingleObject(entered,INFINITE)==WAIT_OBJECT_0,126);
    require(TerminateThread(thread,77),127);record('K',1,tid);
    require(SetEvent(go),128);
    require(WaitForSingleObject(released,INFINITE)==WAIT_OBJECT_0,129);
    DWORD code=0;require(GetExitCodeThread(thread,&code)&&code==77,130);
    require(CloseHandle(thread)&&CloseHandle(go)&&CloseHandle(entered)&&CloseHandle(released),131);
    record('k',1,tid);
}
void entry(void) {
    HANDLE log=CreateFileA("C:\\Temp\\com-apartments.log",GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,0,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);
    require(log!=INVALID_HANDLE_VALUE,1);CloseHandle(log);main_thread=GetCurrentThreadId();record('s',0,main_thread);
    require(CoInitializeEx(0,COINIT_APARTMENTTHREADED)==S_OK,2);
    register_server(1,L"apartment-server.dll",L"Apartment");
    register_server(2,L"free-server.dll",L"Free");
    register_server(3,L"main-server.dll",0);
    register_server(4,L"neutral-server.dll",L"Neutral");
    use_factory(2);
    use_factory(4);
    DWORD tid=0;HANDLE thread=CreateThread(0,0,worker,0,0,&tid);require(thread!=0&&tid!=main_thread,3);
    require(WaitForSingleObject(thread,INFINITE)==WAIT_OBJECT_0,4);
    DWORD code=1;require(GetExitCodeThread(thread,&code)&&code==0,5);require(CloseHandle(thread),6);
#ifdef ORPHAN
    orphan_case();
#endif
#ifdef RUNDOWN
    OracleUnknown *left=0;OracleFactory *factory_left=0;
    require(CoGetClassObject(&classes[1],CLSCTX_INPROC_SERVER,0,&unknown_iid,(void **)&left)==S_OK&&left,132);
    require(left->table->query(left,&factory_iid,(void **)&factory_left)==S_OK&&factory_left,133);
    record('X',2,(DWORD)left);record('Y',2,(DWORD)factory_left);
#endif
    CoUninitialize();record('e',0,0);ExitProcess(0);
}
