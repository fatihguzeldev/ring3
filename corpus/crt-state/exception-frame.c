void __cdecl _EH_prolog(void);
__declspec(dllimport) __declspec(noreturn) void __stdcall ExitProcess(unsigned int);

static unsigned int before, outer_record, after_inner, inner_record, previous, handler_value, state, after;

static void handler(void) {}

__declspec(naked) static void inner(void) {
    __asm {
        lea eax, handler
        call _EH_prolog
        mov ecx, fs:[0]
        mov inner_record, ecx
        mov eax, [ebp-12]
        mov previous, eax
        mov eax, [ebp-8]
        mov handler_value, eax
        mov eax, [ebp-4]
        mov state, eax
        mov ecx, [ebp-12]
        mov fs:[0], ecx
        mov esp, ebp
        pop ebp
        ret
    }
}

__declspec(naked) static void outer(void) {
    __asm {
        lea eax, handler
        call _EH_prolog
        mov ecx, fs:[0]
        mov outer_record, ecx
        call inner
        mov ecx, fs:[0]
        mov after_inner, ecx
        mov ecx, [ebp-12]
        mov fs:[0], ecx
        mov esp, ebp
        pop ebp
        ret
    }
}

void entry(void) {
    __asm { mov eax, fs:[0] }
    __asm { mov before, eax }
    outer();
    __asm { mov eax, fs:[0] }
    __asm { mov after, eax }
    if (before != after || outer_record != after_inner || previous != outer_record) ExitProcess(1);
    if (!inner_record || inner_record == outer_record || handler_value != (unsigned int)handler || state != 0xffffffff) ExitProcess(2);
    ExitProcess(42);
}
