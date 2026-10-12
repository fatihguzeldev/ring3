typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "dword width");

void ring3_clear_bits(_Atomic(u32) *word) {
    __c11_atomic_fetch_and(word, ~32u, __ATOMIC_SEQ_CST);
}
