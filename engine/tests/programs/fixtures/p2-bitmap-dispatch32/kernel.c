typedef unsigned int u32;
_Static_assert(sizeof(u32) == 4, "u32 must be 32 bits");

u32 ring3_take_ready_bit(u32 *pending, u32 allowed) {
    u32 ready = *pending & allowed;
    if (!ready)
        return 32u;
    u32 slot = (u32)__builtin_ctz(ready);
    *pending ^= ready & (0u - ready);
    return slot;
}
