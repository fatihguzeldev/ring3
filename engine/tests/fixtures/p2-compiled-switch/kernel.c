typedef unsigned int u32;

_Static_assert(__CHAR_BIT__ == 8, "byte bits");
_Static_assert(sizeof(u32) == 4, "dword width");

u32 ring3_switch(volatile u32 *state, u32 selector, u32 salt) {
    u32 value = *state;
    switch (selector) {
        case 0:
            value += 7u;
            break;
        case 1:
            value ^= 0xa5a55a5au;
            break;
        case 2:
            value = (value << 5) | (value >> 27);
            break;
        case 3:
            value = value * 3u + 1u;
            break;
        default:
            value -= selector;
            break;
    }
    *state = value;
    return value ^ salt;
}
