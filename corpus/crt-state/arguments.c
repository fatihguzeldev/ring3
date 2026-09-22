typedef struct { int new_mode; } startup_info;
__declspec(dllimport) int __cdecl __getmainargs(int*, char***, char***, int, startup_info*);
__declspec(dllimport) extern char* _acmdln;
__declspec(dllimport) extern int __argc;
__declspec(dllimport) extern char** __argv;
__declspec(dllimport) extern char** _environ;
__declspec(dllimport) int* __cdecl __p___argc(void);
__declspec(dllimport) char*** __cdecl __p___argv(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

void entry(void) {
    int count = 0;
    char** arguments = 0;
    char** environment = 0;
    startup_info info = { 1 };
    if (__getmainargs(&count, &arguments, &environment, 0, &info) != 0) ExitProcess(1);
    if (count != 1) ExitProcess(2);
    if (arguments[0] == 0) ExitProcess(3);
    if (arguments[1] != 0) ExitProcess(4);
    if (environment[0] != 0) ExitProcess(5);
    if (_acmdln == 0) ExitProcess(6);
    if (__argc != count) ExitProcess(7);
    if (__argv != arguments) ExitProcess(8);
    if (_environ != environment) ExitProcess(9);
    int* count_cell = __p___argc();
    char*** vector_cell = __p___argv();
    if (count_cell != &__argc || vector_cell != &__argv) ExitProcess(10);
    *count_cell = 42;
    *vector_cell = 0;
    if (__argc != 42 || __argv != 0) ExitProcess(11);
    *count_cell = count;
    *vector_cell = arguments;
    ExitProcess(42);
}
