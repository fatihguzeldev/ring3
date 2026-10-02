#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static int decimal(const char *text, uint32_t *out) {
    uint32_t value = 0;
    if (*text == '\0') return 0;
    for (; *text != '\0'; ++text) {
        if (*text < '0' || *text > '9') return 0;
        uint32_t digit = (uint32_t)(*text - '0');
        if (value > UINT32_MAX / 10 ||
            (value == UINT32_MAX / 10 && digit > UINT32_MAX % 10)) return 0;
        value = value * 10 + digit;
    }
    *out = value;
    return 1;
}

int main(int argc, char **argv) {
    uint32_t input[13], output[8], zero = 0;
    if (argc < 7 || argc > 14) return 2;
    for (int i = 1; i < argc; ++i) if (!decimal(argv[i], &input[i - 1])) return 2;
    int count = argc - 6;
    for (int i = 0; i < count; ++i) {
        uint32_t value = input[i + 5];
        value += input[0];
        value -= input[1];
        value &= input[2];
        value |= input[3];
        value ^= input[4];
        output[i] = value;
        if (value == 0) ++zero;
    }
    printf("{\"values\":[");
    for (int i = 0; i < count; ++i) printf("%s%" PRIu32, i ? "," : "", output[i]);
    printf("],\"markers\":[");
    for (int i = 0; i < count; ++i) printf("%s%u", i ? "," : "", output[i] == 0 ? 1u : 2u);
    printf("],\"zero\":%" PRIu32 ",\"nonzero\":%" PRIu32 "}\n", zero, (uint32_t)count - zero);
    return 0;
}
