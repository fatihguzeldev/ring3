typedef unsigned int u32;
typedef u32 v4u32 __attribute__((vector_size(16)));

u32 ring3_reduce(v4u32 *restrict output,
                 const v4u32 *restrict input,
                 const v4u32 *restrict coefficients,
                 u32 count) {
    v4u32 accumulator = coefficients[0];
    const v4u32 scale = coefficients[1];
    for (u32 i = 0; i < count; ++i) {
        accumulator += input[i] * scale;
        output[i] = accumulator;
    }
    return accumulator[0] ^ accumulator[1] ^ accumulator[2] ^ accumulator[3];
}
