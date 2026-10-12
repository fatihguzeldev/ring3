typedef unsigned int u32;
typedef double v2df __attribute__((vector_size(16)));

extern u32 ring3_reduce(v2df *restrict output, const v2df *restrict input,
                       const v2df *restrict coefficients, u32 count,
                       double *restrict scalar_output);

_Alignas(16) const v2df ring3_input[3] = {
    {1.0, 4.0}, {-0.5, -2.0}, {2.0, 1.0}
};
_Alignas(16) const v2df ring3_coefficients[4] = {
    {1.0, -2.0}, {2.0, 0.5}, {0.25, -0.25}, {3.0, 0.5}
};
_Alignas(16) v2df ring3_output[3];
double ring3_scalar_output;
volatile u32 ring3_completion[2];
int _fltused = 0;

void ring3_entry(void) {
    ring3_completion[0] = ring3_reduce(ring3_output, ring3_input,
                                     ring3_coefficients, 3,
                                     &ring3_scalar_output);
    ring3_completion[1] = 0x52494e47u;
    for (;;) {
    }
}
