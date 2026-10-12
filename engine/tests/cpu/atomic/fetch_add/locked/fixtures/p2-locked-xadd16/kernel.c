typedef unsigned short u16;
_Static_assert(sizeof(u16) == 2, "word width");

u16 ring3_fetch_add(_Atomic(u16) *cell, u16 increment) {
    return __c11_atomic_fetch_add(cell, increment, __ATOMIC_RELAXED);
}
