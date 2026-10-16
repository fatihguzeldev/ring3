#include "contracts.h"
static void registration(const WCHAR *name,const WCHAR *value,const WCHAR *model) {
    HKEY key=0;DWORD disposition=0,units=0;require(RegCreateKeyExW(HKEY_CURRENT_USER,name,0,0,0,KEY_ALL_ACCESS,0,&key,&disposition)==ERROR_SUCCESS,1);while(value[units])++units;require(RegSetValueExW(key,0,0,REG_SZ,(const BYTE *)value,(units+1)*2)==ERROR_SUCCESS,2);
    if(model){units=0;while(model[units])++units;require(RegSetValueExW(key,L"ThreadingModel",0,REG_SZ,(const BYTE *)model,(units+1)*2)==ERROR_SUCCESS,3);}require(RegCloseKey(key)==ERROR_SUCCESS,4);
}
void entry(void) {
    HANDLE log=CreateFileA("C:\\Temp\\com-rpc.log",GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,0,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);require(log!=INVALID_HANDLE_VALUE,5);CloseHandle(log);DWORD caller=GetCurrentThreadId();record('b',3,caller);require(CoInitializeEx(0,COINIT_MULTITHREADED)==S_OK,6);
    registration(L"Software\\Classes\\CLSID\\{57383601-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",L"rpc-source.dll",L"Apartment");registration(L"Software\\Classes\\CLSID\\{57383602-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",L"rpc-proxy.dll",L"Both");registration(L"Software\\Classes\\Interface\\{57383610-9461-4d83-9146-2743d4e9ae01}\\ProxyStubClsid32",L"{57383602-9461-4d83-9146-2743d4e9ae01}",0);
    Value *value=0;require(CoCreateInstance(&source_class,0,CLSCTX_INPROC_SERVER,&value_iid,(void **)&value)==S_OK&&value,7);DWORD actual=0,owner=0;require(value->table->get(value,0,&actual,&owner)==S_OK&&actual==0x2468ace0&&owner!=caller&&owner,8);record('o',3,owner);
    IUnknown *unknown=0,*again=0;require(value->table->query(value,&unknown_iid,(void **)&unknown)==S_OK&&unknown,9);require(IUnknown_QueryInterface(unknown,&unknown_iid,(void **)&again)==S_OK&&again==unknown,10);IUnknown_Release(again);IUnknown_Release(unknown);
    actual=1;require(value->table->get(value,1,&actual,&owner)==E_FAIL&&actual==0&&owner!=caller,11);value->table->release(value);CoFreeUnusedLibraries();CoUninitialize();record('e',3,0);ExitProcess(0);
}
