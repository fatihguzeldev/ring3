typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "dword width");

u32 ring3_update_flags(_Atomic(u32) *word) {
    __c11_atomic_fetch_or(word, 0x80010000u, __ATOMIC_SEQ_CST);
    u32 set = __c11_atomic_load(word, __ATOMIC_SEQ_CST);
    __c11_atomic_fetch_and(word, ~0x00020040u, __ATOMIC_SEQ_CST);
    u32 cleared = __c11_atomic_load(word, __ATOMIC_SEQ_CST);
    __c11_atomic_fetch_xor(word, 0x00400800u, __ATOMIC_SEQ_CST);
    u32 toggled = __c11_atomic_load(word, __ATOMIC_SEQ_CST);
    return set ^ cleared ^ toggled;
}
