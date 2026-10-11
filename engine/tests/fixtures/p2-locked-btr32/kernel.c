typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "dword width");
_Static_assert(sizeof(long) == 4, "windows long width");

unsigned char _interlockedbittestandreset(long volatile *, long);
#pragma intrinsic(_interlockedbittestandreset)

u32 ring3_reset_bit(long volatile *word) {
    return _interlockedbittestandreset(word, 5);
}
