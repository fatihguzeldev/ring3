#define WIN32_LEAN_AND_MEAN
#define _WIN32_WINNT 0x0601
#include <windows.h>
#include <stdio.h>
struct notice { UINT id,code; BOOL captured,focused,dropped; };
static struct notice notices[64];
static UINT count,rows;
static LRESULT CALLBACK parent_proc(HWND window,UINT message,WPARAM word,LPARAM value) {
    if(message==WM_COMMAND&&value&&count<64) {
        HWND control=(HWND)value;
        struct notice *n=&notices[count++];
        n->id=LOWORD(word);n->code=HIWORD(word);
        n->captured=GetCapture()==control;n->focused=GetFocus()==control;
        n->dropped=n->id==30?(BOOL)SendMessageW(control,CB_GETDROPPEDSTATE,0,0):FALSE;
    }
    return DefWindowProcW(window,message,word,value);
}
static void start(const char *name) {
    if(rows++)putchar(',');printf("{\"case\":\"%s\"",name);
}
static void events(void) {
    UINT i;printf(",\"events\":[");
    for(i=0;i<count;++i) {if(i)putchar(',');printf("[%u,%u,%u,%u,%u]",notices[i].id,notices[i].code,notices[i].captured,notices[i].focused,notices[i].dropped);}
    printf("]}");count=0;
}
static HWND child(HWND parent,const WCHAR *class_name,DWORD style,int id,int y,int height) {
    HWND h=CreateWindowExW(0,class_name,L"item",WS_CHILD|WS_VISIBLE|WS_TABSTOP|style,10,y,160,height,parent,(HMENU)(INT_PTR)id,GetModuleHandleW(NULL),NULL);
    if(!h)ExitProcess(2);
    SendMessageW(h,WM_SETFONT,(WPARAM)GetStockObject(DEFAULT_GUI_FONT),FALSE);
    return h;
}
static void button_row(const char *name,HWND button) {
    start(name);printf(",\"state\":%ld,\"check\":%ld,\"capture\":%u,\"focus\":%u",(long)SendMessageW(button,BM_GETSTATE,0,0),(long)SendMessageW(button,BM_GETCHECK,0,0),GetCapture()==button,GetFocus()==button);events();
}
int main(void) {
    WNDCLASSW wc={0};HWND parent,button,list,combo;COMBOBOXINFO info={0};RECT client;
    LRESULT result;int height;HINSTANCE instance=GetModuleHandleW(NULL);
    setvbuf(stdout,NULL,_IONBF,0);
    wc.lpfnWndProc=parent_proc;wc.hInstance=instance;wc.lpszClassName=L"Ring3ControlInput";
    if(!RegisterClassW(&wc))return 2;
    parent=CreateWindowExW(0,wc.lpszClassName,L"controls",WS_OVERLAPPEDWINDOW|WS_VISIBLE,0,0,320,420,NULL,NULL,instance,NULL);
    if(!parent)return 2;
    SetActiveWindow(parent);
    button=child(parent,L"Button",BS_AUTOCHECKBOX,10,10,24);
    list=child(parent,L"ListBox",LBS_NOTIFY|WS_BORDER,20,50,80);
    combo=child(parent,L"ComboBox",CBS_DROPDOWNLIST,30,150,120);
    SendMessageW(list,LB_ADDSTRING,0,(LPARAM)L"first");SendMessageW(list,LB_ADDSTRING,0,(LPARAM)L"second");
    SendMessageW(combo,CB_ADDSTRING,0,(LPARAM)L"first");SendMessageW(combo,CB_ADDSTRING,0,(LPARAM)L"second");
    printf("{\"schema\":1,\"component\":\"standard-control-input\",\"pointerBytes\":%u,\"rows\":[",(unsigned)sizeof(void*));
    SetFocus(parent);count=0;
    SendMessageW(button,WM_LBUTTONDOWN,MK_LBUTTON,MAKELPARAM(5,5));button_row("button-down",button);
    SendMessageW(button,WM_LBUTTONUP,0,MAKELPARAM(5,5));button_row("button-up-inside",button);
    SendMessageW(button,WM_LBUTTONDOWN,MK_LBUTTON,MAKELPARAM(5,5));count=0;
    SendMessageW(button,WM_MOUSEMOVE,MK_LBUTTON,MAKELPARAM(-2,5));button_row("button-drag-outside",button);
    SendMessageW(button,WM_LBUTTONUP,0,MAKELPARAM(-2,5));button_row("button-up-outside",button);
    SendMessageW(button,WM_KEYDOWN,VK_SPACE,0);button_row("button-space-down",button);
    SendMessageW(button,WM_KEYUP,VK_SPACE,0);button_row("button-space-up",button);
    SendMessageW(button,WM_LBUTTONDOWN,MK_LBUTTON,MAKELPARAM(5,5));SetFocus(parent);button_row("button-lost-focus",button);
    EnableWindow(button,FALSE);count=0;SendMessageW(button,WM_LBUTTONDOWN,MK_LBUTTON,MAKELPARAM(5,5));SendMessageW(button,WM_LBUTTONUP,0,MAKELPARAM(5,5));button_row("button-disabled",button);
    EnableWindow(button,TRUE);SetFocus(parent);count=0;
    height=(int)SendMessageW(list,LB_GETITEMHEIGHT,0,0);if(height<=0)return 2;
    SendMessageW(list,WM_LBUTTONDOWN,MK_LBUTTON,MAKELPARAM(4,height+3));start("list-down");printf(",\"selected\":%ld,\"capture\":%u,\"itemHeight\":%d",(long)SendMessageW(list,LB_GETCURSEL,0,0),GetCapture()==list,height);events();
    SendMessageW(list,WM_LBUTTONUP,0,MAKELPARAM(4,height+3));start("list-up");printf(",\"selected\":%ld,\"capture\":%u",(long)SendMessageW(list,LB_GETCURSEL,0,0),GetCapture()==list);events();
    SetFocus(parent);count=0;
    result=SendMessageW(combo,CB_SHOWDROPDOWN,TRUE,0);start("combo-show");printf(",\"return\":%ld,\"dropped\":%ld,\"capture\":%u,\"focus\":%u",(long)result,(long)SendMessageW(combo,CB_GETDROPPEDSTATE,0,0),GetCapture()==combo,GetFocus()==combo);events();
    info.cbSize=sizeof(info);if(!GetComboBoxInfo(combo,&info))return 2;
    height=(int)SendMessageW(combo,CB_GETITEMHEIGHT,0,0);GetClientRect(info.hwndList,&client);
    count=0;SendMessageW(info.hwndList,WM_LBUTTONDOWN,MK_LBUTTON,MAKELPARAM(4,height+3));
    SendMessageW(info.hwndList,WM_LBUTTONUP,0,MAKELPARAM(4,height+3));
    start("combo-list-click");printf(",\"selected\":%ld,\"dropped\":%ld,\"listCapture\":%u,\"comboCapture\":%u",(long)SendMessageW(combo,CB_GETCURSEL,0,0),(long)SendMessageW(combo,CB_GETDROPPEDSTATE,0,0),GetCapture()==info.hwndList,GetCapture()==combo);events();
    SendMessageW(combo,CB_SHOWDROPDOWN,TRUE,0);count=0;
    result=SendMessageW(combo,CB_SHOWDROPDOWN,FALSE,0);start("combo-hide");printf(",\"return\":%ld,\"dropped\":%ld",(long)result,(long)SendMessageW(combo,CB_GETDROPPEDSTATE,0,0));events();
    printf("]}\n");DestroyWindow(parent);return 0;
}
