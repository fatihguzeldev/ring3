typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "dword width");

u32 ring3_collect_flag_updates(_Atomic(u32) *word, u32 set_mask, u32 keep_mask, u32 flip_mask) {
    u32 before_set = __c11_atomic_fetch_or(word, set_mask, __ATOMIC_SEQ_CST);
    u32 before_clear = __c11_atomic_fetch_and(word, keep_mask, __ATOMIC_SEQ_CST);
    u32 before_toggle = __c11_atomic_fetch_xor(word, flip_mask, __ATOMIC_SEQ_CST);
    return before_set + before_clear + before_toggle;
}
