typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "dword width");

u32 ring3_update_dynamic_flags(_Atomic(u32) *word, u32 set_mask, u32 keep_mask, u32 flip_mask) {
    __c11_atomic_fetch_or(word, set_mask, __ATOMIC_SEQ_CST);
    u32 set = __c11_atomic_load(word, __ATOMIC_SEQ_CST);
    __c11_atomic_fetch_and(word, keep_mask, __ATOMIC_SEQ_CST);
    u32 cleared = __c11_atomic_load(word, __ATOMIC_SEQ_CST);
    __c11_atomic_fetch_xor(word, flip_mask, __ATOMIC_SEQ_CST);
    u32 toggled = __c11_atomic_load(word, __ATOMIC_SEQ_CST);
    return set ^ cleared ^ toggled;
}
