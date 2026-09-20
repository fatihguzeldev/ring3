__declspec(dllimport) void __cdecl __set_app_type(int);
__declspec(dllimport) int* __cdecl __p__fmode(void);
__declspec(dllimport) int* __cdecl __p__commode(void);
__declspec(dllimport) extern int _adjust_fdiv;
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

volatile unsigned int input = 32;

void entry(void) {
    __set_app_type(2);
    int* fmode = __p__fmode();
    int* commode = __p__commode();
    if (*fmode != 0x4000) ExitProcess(1);
    *fmode = input | 3;
    *commode = 7;
    ExitProcess(*fmode + *commode + _adjust_fdiv);
}
