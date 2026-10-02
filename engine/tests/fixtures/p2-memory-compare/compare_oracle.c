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
    uint32_t threshold, mask, input[24];
    if (argc < 5 || argc > 27 || (argc - 3) % 2 != 0 ||
        !decimal(argv[1], &threshold) || !decimal(argv[2], &mask)) return 2;
    for (int i = 3; i < argc; ++i) {
        if (!decimal(argv[i], &input[i - 3])) return 2;
    }
    uint32_t accepted = 0, below = 0, zero = 0, last_read = 0;
    for (int i = 0; i < argc - 3; i += 2) {
        last_read = input[i];
        if (input[i] < threshold) {
            ++below;
        } else {
            last_read = input[i + 1];
            if ((input[i + 1] & mask) == 0) ++zero; else ++accepted;
        }
    }
    printf("{\"accepted\":%" PRIu32 ",\"below\":%" PRIu32
           ",\"zero\":%" PRIu32 ",\"last_read\":%" PRIu32 "}\n",
           accepted, below, zero, last_read);
    return 0;
}
