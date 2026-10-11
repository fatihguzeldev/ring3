typedef unsigned char u8;
typedef unsigned short u16;

_Static_assert(sizeof(u8) == 1, "byte width");
_Static_assert(sizeof(u16) == 2, "word width");

u8 ring3_compare_exchange(_Atomic(u16) *cell, u16 *expected, u16 replacement) {
    return __c11_atomic_compare_exchange_strong(
        cell, expected, replacement, __ATOMIC_RELAXED, __ATOMIC_RELAXED);
}
