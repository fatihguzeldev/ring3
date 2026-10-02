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
    uint32_t first, second;
    if (argc != 3 || !decimal(argv[1], &first) || !decimal(argv[2], &second))
        return 2;
    uint32_t sum = first + second;
    printf("{\"sum\":%" PRIu32 "}\n", sum);
    return 0;
}
