typedef unsigned int u32;
typedef double v2df __attribute__((vector_size(16)));

extern u32 ring3_reduce(v2df *restrict output, const v2df *restrict input,
                       const v2df *restrict coefficients, u32 count,
                       double *restrict scalar_output);
__declspec(dllimport) u32 __stdcall GetLastError(void);
__declspec(dllimport) void __stdcall SetLastError(u32 error);

_Alignas(16) const v2df ring3_input[3] = {
    {1.0, 4.0}, {-0.5, -2.0}, {2.0, 1.0}
};
_Alignas(16) const v2df ring3_coefficients[4] = {
    {1.0, -2.0}, {2.0, 0.5}, {0.25, -0.25}, {3.0, 0.5}
};
_Alignas(16) v2df ring3_first_output[3];
_Alignas(16) v2df ring3_second_output[3];
_Alignas(16) double ring3_first_scalar;
_Alignas(16) double ring3_second_scalar;
volatile u32 ring3_result[4];
int _fltused = 0;

void ring3_entry(void) {
    ring3_result[0] = ring3_reduce(ring3_first_output, ring3_input,
                                 ring3_coefficients, 3, &ring3_first_scalar);
    SetLastError(0xf1234567u);
    u32 got = GetLastError();
    ring3_result[1] = got;
    ring3_result[2] = ring3_reduce(ring3_second_output, ring3_first_output,
                                 ring3_coefficients, got & 3u,
                                 &ring3_second_scalar);
    ring3_result[3] = 0x52494e47u;
    for (;;) {
    }
}
