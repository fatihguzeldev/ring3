typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "dword width");
_Static_assert(sizeof(long) == 4, "windows long width");

unsigned char _interlockedbittestandset(long volatile *, long);
#pragma intrinsic(_interlockedbittestandset)

u32 ring3_set_bit(long volatile *word) {
    return _interlockedbittestandset(word, 5);
}
