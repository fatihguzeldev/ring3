#include <windows.h>
#include <stdio.h>
_Static_assert(sizeof(void *) == 4, "this oracle requires a native 32-bit build");
static void row(const char *operation, BOOL ok, DWORD error, DWORD flags) {
    printf("{\"operation\":\"%s\",\"ok\":%u,\"error\":%lu,\"flags\":%lu}\n",operation,ok!=0,(unsigned long)error,(unsigned long)flags);
}
int main(void) {
    HANDLE process=GetCurrentProcess(), handle=CreateEventW(NULL,FALSE,FALSE,NULL), copy=NULL;
    DWORD flags=0;
    if(!handle)return 2;
    SetLastError(0);BOOL ok=SetHandleInformation(handle,3,3);row("set-protected",ok,GetLastError(),0);
    SetLastError(0);ok=GetHandleInformation(handle,&flags);row("get-protected",ok,GetLastError(),flags);
    SetLastError(0);ok=CloseHandle(handle);row("close-protected",ok,GetLastError(),0);
    SetLastError(0);ok=DuplicateHandle(process,handle,process,&copy,0,FALSE,DUPLICATE_SAME_ACCESS|DUPLICATE_CLOSE_SOURCE);row("duplicate-close-protected",ok,GetLastError(),0);
    SetLastError(0);ok=GetHandleInformation(handle,&flags);row("source-after-duplicate",ok,GetLastError(),flags);
    if(copy){SetLastError(0);ok=GetHandleInformation(copy,&flags);row("copy-flags",ok,GetLastError(),flags);CloseHandle(copy);}
    SetHandleInformation(handle,3,0);CloseHandle(handle);
    SetLastError(0);ok=GetHandleInformation(process,&flags);row("pseudo-process-flags",ok,GetLastError(),flags);
    return 0;
}
