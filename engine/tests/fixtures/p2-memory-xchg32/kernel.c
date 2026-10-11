typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "DWORD width");

u32 ring3_exchange(_Atomic(u32) *cell, u32 replacement) {
    return __c11_atomic_exchange(cell, replacement, __ATOMIC_SEQ_CST);
}
