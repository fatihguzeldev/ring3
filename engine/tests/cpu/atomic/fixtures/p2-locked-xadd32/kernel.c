typedef unsigned int u32;

_Static_assert(sizeof(u32) == 4, "DWORD width");

u32 ring3_fetch_add(_Atomic(u32) *cell, u32 increment) {
    return __c11_atomic_fetch_add(cell, increment, __ATOMIC_RELAXED);
}
