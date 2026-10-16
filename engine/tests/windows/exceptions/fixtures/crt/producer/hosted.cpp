#include <stdio.h>
#include <stdlib.h>
#include <setjmp.h>

extern "C" __declspec(dllimport) void* __stdcall LoadLibraryA(const char*);
extern "C" __declspec(dllimport) void* __stdcall GetProcAddress(void*, const char*);
extern "C" __declspec(dllimport) int __stdcall FreeLibrary(void*);
extern "C" __declspec(dllimport) unsigned __stdcall FlsAlloc(void (__stdcall *)(void*));
extern "C" __declspec(dllimport) int __stdcall FlsFree(unsigned);
extern "C" __declspec(dllimport) int __stdcall FlsSetValue(unsigned, void*);
extern "C" __declspec(dllimport) void* __stdcall FlsGetValue(unsigned);
extern "C" __declspec(dllimport) void* __stdcall CreateThread(
    void*, unsigned, unsigned (__stdcall *)(void*), void*, unsigned, unsigned*);
extern "C" __declspec(dllimport) unsigned __stdcall WaitForSingleObject(void*, unsigned);
extern "C" __declspec(dllimport) int __stdcall GetExitCodeThread(void*, unsigned*);
extern "C" __declspec(dllimport) int __stdcall CloseHandle(void*);

__declspec(thread) volatile int tls_value = 17;
static volatile int state;
static void record(const char* value) {
    FILE* file = fopen("C:\\Temp\\hosted.txt", "ab");
    if (file) { fwrite(value, 1, 1, file); fclose(file); }
}
struct Global { Global() { state = 11; } ~Global() { record("G"); } } global;
struct Local { ~Local() { ++state; } };
static void ending() { record("A"); }
static void __stdcall fls_drop(void* value) { record((const char*)value); }
static jmp_buf jump;
__declspec(noinline) static void jump_back() { longjmp(jump, 7); }

typedef int (__cdecl *Value)();
struct ChildContext { unsigned fls; Value dll_child_value; };
static unsigned __stdcall run_child(void* parameter) {
    ChildContext* context = (ChildContext*)parameter;
    if (tls_value != 17 || context->dll_child_value() != 41) return 103;
    tls_value = 23;
    char* data = (char*)malloc(32);
    if (!data) return 104;
    data[0] = 'T';
    record(data);
    free(data);
    if (!FlsSetValue(context->fls, (void*)"Y") || !FlsGetValue(context->fls)) return 105;
    return tls_value == 23 ? 42 : 106;
}

extern "C" int main(int argc, char** argv) {
    if (state != 11 || tls_value != 17 || argc < 1 || !argv[0]) return 91;
    char* data = (char*)malloc(32);
    if (!data) return 92;
    data[0] = 'M';
    free(data);
    int caught = 0;
    try { Local local; throw 37; } catch (int value) { caught = value; }
    if (caught != 37 || state != 12) return 93;
    int result = setjmp(jump);
    if (!result) jump_back();
    if (result != 7) return 94;
    FILE* file = fopen("C:\\Temp\\hosted.txt", "wb");
    if (!file) return 95;
    fwrite("M", 1, 1, file);
    fclose(file);
    void* dll = LoadLibraryA("hosted-dll.dll");
    if (!dll) return 97;
    typedef void (__cdecl *Throw)();
    Value value = (Value)GetProcAddress(dll, "dll_value");
    Value child_value = (Value)GetProcAddress(dll, "dll_child_value");
    Throw raising = (Throw)GetProcAddress(dll, "dll_throw");
    if (!value || !child_value || !raising || value() != 29) return 98;
    int crossed = 0;
    try { raising(); } catch (int value) { crossed = value; record("C"); }
    if (crossed != 43) return 99;

    unsigned child_fls = FlsAlloc(fls_drop);
    if (child_fls == 0xffffffffU) return 107;
    tls_value = 71;
    ChildContext context = {child_fls, child_value};
    unsigned child_id = 0, child_exit = 0;
    void* child = CreateThread(0, 0, run_child, &context, 0, &child_id);
    if (!child || child_id == 0 || child_id == 1) return 108;
    if (WaitForSingleObject(child, 0xffffffffU) != 0) return 109;
    if (!GetExitCodeThread(child, &child_exit) || child_exit != 42) return 110;
    if (!CloseHandle(child) || !FlsFree(child_fls)) return 111;
    if (tls_value != 71 || value() != 29) return 112;
    record("J");
    if (!FreeLibrary(dll)) return 99;

    unsigned fls = FlsAlloc(fls_drop);
    if (fls == 0xffffffffU) return 100;
    if (!FlsSetValue(fls, (void*)"F") || !FlsGetValue(fls) || !FlsFree(fls)) return 101;
    fls = FlsAlloc(fls_drop);
    if (fls == 0xffffffffU || !FlsSetValue(fls, (void*)"X")) return 102;
    if (atexit(ending) != 0) return 96;
    return 0;
}
