#include <stdio.h>
#include <stdlib.h>

__declspec(thread) volatile int dll_tls = 29;
static void record(const char* value) {
    FILE* file = fopen("C:\\Temp\\hosted.txt", "ab");
    if (file) { fwrite(value, 1, 1, file); fclose(file); }
}
struct DllGlobal { DllGlobal() { record("D"); } ~DllGlobal() { record("E"); } } dll_global;
struct DllLocal { ~DllLocal() { record("L"); } };
extern "C" int __cdecl dll_value() { return dll_tls; }
extern "C" int __cdecl dll_child_value() {
    if (dll_tls != 29) return -1;
    dll_tls = 41;
    return dll_tls;
}
extern "C" void __cdecl dll_throw() { DllLocal local; throw 43; }
extern "C" int __stdcall DllMain(void*, unsigned reason, void*) {
    if (reason == 2) record(dll_tls == 29 ? "a" : "!");
    if (reason == 3) record(dll_tls == 41 ? "b" : "!");
    return 1;
}
