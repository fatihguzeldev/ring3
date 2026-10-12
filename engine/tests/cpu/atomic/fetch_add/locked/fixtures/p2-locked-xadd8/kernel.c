typedef unsigned char u8;
_Static_assert(sizeof(u8) == 1, "byte width");

u8 ring3_fetch_add(_Atomic(u8) *cell, u8 increment) {
    return __c11_atomic_fetch_add(cell, increment, __ATOMIC_RELAXED);
}
