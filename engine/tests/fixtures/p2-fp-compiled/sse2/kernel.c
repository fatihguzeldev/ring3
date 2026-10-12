typedef unsigned int u32;
typedef double v2df __attribute__((vector_size(16)));

u32 ring3_reduce(v2df *restrict output, const v2df *restrict input,
                 const v2df *restrict coefficients, u32 count,
                 double *restrict scalar_output) {
    v2df accumulator = coefficients[0];
    const v2df scale = coefficients[1];
    const v2df bias = coefficients[2];
    for (u32 index = 0; index < count; ++index) {
        accumulator += input[index] * scale;
        output[index] = accumulator + bias;
    }
    *scalar_output = (accumulator[0] + accumulator[1]) * coefficients[3][0]
                     + coefficients[3][1];
    return count;
}
