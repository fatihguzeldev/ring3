unsigned int state;

int __stdcall attach(void* module, unsigned int reason, void* reserved) {
    if (module != (void*)0x50000000) return 0;
    if (reason != 1) return 0;
    if (reserved == 0) return 0;
    state = 40;
    return 1;
}

unsigned int __cdecl value(void) { return state + 1; }
unsigned int __cdecl ordinal_value(void) { return state + 2; }
