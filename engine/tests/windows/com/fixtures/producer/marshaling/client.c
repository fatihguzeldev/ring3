#include "contracts.h"
struct Value {const ValueTable *table;};
static void registration(const WCHAR *key_name,const WCHAR *path,const WCHAR *model) {
    HKEY key=0;DWORD disposition=0,units=0;
    require(RegCreateKeyExW(HKEY_CURRENT_USER,key_name,0,0,0,KEY_ALL_ACCESS,0,&key,&disposition)==ERROR_SUCCESS,1);
    while(path[units])++units;
    require(RegSetValueExW(key,0,0,REG_SZ,(const BYTE *)path,(units+1)*2)==ERROR_SUCCESS,2);
    units=0;while(model[units])++units;
    require(RegSetValueExW(key,L"ThreadingModel",0,REG_SZ,(const BYTE *)model,(units+1)*2)==ERROR_SUCCESS,3);
    require(RegCloseKey(key)==ERROR_SUCCESS,4);
}
void entry(void) {
    HANDLE log=CreateFileA("C:\\Temp\\com-marshaling.log",GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,0,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,0);require(log!=INVALID_HANDLE_VALUE,5);CloseHandle(log);
    DWORD caller=GetCurrentThreadId();record('b',2,caller);
    require(CoInitializeEx(0,COINIT_MULTITHREADED)==S_OK,6);
    registration(L"Software\\Classes\\CLSID\\{57383501-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",L"marshal-source.dll",L"Apartment");
    registration(L"Software\\Classes\\CLSID\\{57383502-9461-4d83-9146-2743d4e9ae01}\\InprocServer32",L"marshal-copy.dll",L"Both");
    Value *value=0;require(CoCreateInstance(&source_class,0,CLSCTX_INPROC_SERVER,&value_iid,(void **)&value)==S_OK&&value,7);
    record('p',2,(DWORD)value);DWORD actual=0,owner=0;require(value->table->get(value,&actual,&owner)==S_OK&&actual==0x13579bdf&&owner==caller,8);
    Value *identity=0;require(value->table->query(value,&unknown_iid,(void **)&identity)==S_OK&&identity==value,9);identity->table->release(identity);
    require(value->table->release(value)==0,10);
    value=(void *)1;require(CoCreateInstance(&source_class,0,CLSCTX_INPROC_SERVER,&fail_iid,(void **)&value)==E_FAIL&&value==0,11);
    record('f',2,0);CoFreeUnusedLibraries();CoUninitialize();record('e',2,0);ExitProcess(0);
}
