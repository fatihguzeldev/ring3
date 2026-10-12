typedef unsigned char u8;
typedef unsigned int u32;

_Static_assert(sizeof(u8) == 1, "byte width");
_Static_assert(sizeof(u32) == 4, "DWORD width");

u32 ring3_compare_exchange(_Atomic(u8) *cell, u8 *expected, u8 replacement) {
    return __c11_atomic_compare_exchange_strong(
        cell, expected, replacement, __ATOMIC_RELAXED, __ATOMIC_RELAXED);
}
