typedef unsigned int u32;

u32 ring3_reduce(volatile const u32 *ready, u32 limit, u32 seed) {
    while (limit && *ready == 0) {
        __builtin_ia32_pause();
        --limit;
    }
    return seed + limit;
}
