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
    uint32_t value, input[36];
    if (argc < 8 || argc > 38 || (argc - 2) % 6 != 0 ||
        !decimal(argv[1], &value)) return 2;
    for (int i = 2; i < argc; ++i) {
        if (!decimal(argv[i], &input[i - 2])) return 2;
    }
    uint32_t branch_sum = 0, last_branch = 0, below = 0, above = 0;
    for (int i = 0; i < argc - 2; i += 6) {
        value += input[i];
        value ^= input[i + 1];
        value &= input[i + 2];
        value |= input[i + 3];
        value -= input[i + 4];
        last_branch = value < input[i + 5] ? 2 : 1;
        branch_sum += last_branch;
        if (last_branch == 2) ++below; else ++above;
    }
    printf("{\"value\":%" PRIu32 ",\"branch_sum\":%" PRIu32
           ",\"last_branch\":%" PRIu32 ",\"below\":%" PRIu32
           ",\"above\":%" PRIu32 "}\n", value, branch_sum, last_branch, below, above);
    return 0;
}
