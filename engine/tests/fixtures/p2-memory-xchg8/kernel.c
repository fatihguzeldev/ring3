typedef unsigned char u8;
typedef unsigned int u32;

_Static_assert(sizeof(u8) == 1, "u8 must occupy one byte");
_Static_assert(sizeof(u32) == 4, "u32 must occupy four bytes");

u32 ring3_exchange_byte(_Atomic(u8) *cell, u32 desired) {
    return (u32)__c11_atomic_exchange(cell, (u8)desired, 5);
}
