typedef unsigned int u32;

_Static_assert(sizeof(u32) == 4, "expected dword count");
_Static_assert(sizeof(long double) == 16, "expected padded extended80");
_Static_assert(__LDBL_MANT_DIG__ == 64, "expected extended80 precision");
_Static_assert(__LDBL_MAX_EXP__ == 16384, "expected extended80 exponent");

long double ring3_reduce(const long double *values, u32 count) {
    volatile long double total = 0.0L;
    for (u32 index = 0; index < count; ++index) {
        total = total + values[index];
    }
    return total;
}
