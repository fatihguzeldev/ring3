typedef unsigned short u16;
typedef unsigned int u32;

_Static_assert(sizeof(u16) == 2, "u16 must occupy two bytes");
_Static_assert(sizeof(u32) == 4, "u32 must occupy four bytes");

u32 ring3_exchange_word(_Atomic(u16) *cell, u32 desired) {
    return (u32)__c11_atomic_exchange(cell, (u16)desired, 5);
}
