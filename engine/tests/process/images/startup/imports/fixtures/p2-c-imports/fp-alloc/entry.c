typedef unsigned int u32;
typedef double v2df __attribute__((vector_size(16)));

extern u32 ring3_reduce(v2df *restrict output, const v2df *restrict input,
                       const v2df *restrict coefficients, u32 count,
                       double *restrict scalar_output);
__declspec(dllimport) void *__stdcall VirtualAlloc(void *address, u32 size,
                                                  u32 allocation_type,
                                                  u32 protection);
__declspec(dllimport) u32 __stdcall VirtualFree(void *address, u32 size,
                                              u32 free_type);

_Alignas(16) const v2df ring3_input[3] = {
    {1.0, 4.0}, {-0.5, -2.0}, {2.0, 1.0}
};
_Alignas(16) const v2df ring3_coefficients[4] = {
    {1.0, -2.0}, {2.0, 0.5}, {0.25, -0.25}, {3.0, 0.5}
};
_Alignas(16) v2df ring3_output[3];
_Alignas(16) double ring3_scalar;
volatile u32 ring3_result[4];
int _fltused = 0;

void ring3_entry(void) {
    v2df *output = VirtualAlloc((void *)0, 4096, 0x3000, 4);
    double *scalar = (double *)((char *)output + 48);
    ring3_result[0] = (u32)output;
    ring3_result[1] = ring3_reduce(output, ring3_input, ring3_coefficients,
                                 3, scalar);
    ring3_output[0] = output[0];
    ring3_output[1] = output[1];
    ring3_output[2] = output[2];
    ring3_scalar = *scalar;
    ring3_result[2] = VirtualFree(output, 0, 0x8000);
    ring3_result[3] = 0x52494e47u;
    for (;;) {
    }
}
