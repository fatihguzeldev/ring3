#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static int decimal(const char *text, uint32_t maximum, uint32_t *out) {
    uint32_t value = 0;
    if (*text == '\0') return 0;
    for (; *text != '\0'; ++text) {
        if (*text < '0' || *text > '9') return 0;
        uint32_t digit = (uint32_t)(*text - '0');
        if (value > maximum / 10 ||
            (value == maximum / 10 && digit > maximum % 10)) return 0;
        value = value * 10 + digit;
    }
    *out = value;
    return 1;
}

int main(int argc, char **argv) {
    uint32_t kind, left, right;
    if (argc != 4 || !decimal(argv[1], 1, &kind) ||
        !decimal(argv[2], UINT32_MAX, &left) ||
        !decimal(argv[3], UINT32_MAX, &right)) return 2;
    uint32_t result = kind == 0 ? left + right : left - right;
    printf("{\"result\":%" PRIu32 "}\n", result);
    return 0;
}
