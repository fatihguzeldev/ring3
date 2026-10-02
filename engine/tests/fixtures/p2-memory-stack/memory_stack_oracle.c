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
    uint32_t first, second, saved, seed;
    if (argc != 5 || !decimal(argv[1], &first) || !decimal(argv[2], &second) ||
        !decimal(argv[3], &saved) || !decimal(argv[4], &seed)) return 2;
    uint32_t input[2] = { first, second }, output[2], scratch = saved;
    uint32_t stack[3], count = 0;
    stack[count++] = scratch;
    stack[count++] = input[0];
    stack[count++] = input[1];
    output[0] = stack[--count];
    output[1] = stack[--count];
    scratch = stack[--count];
    uint32_t sum = output[0] + seed;
    printf("{\"first\":%" PRIu32 ",\"second\":%" PRIu32
           ",\"saved\":%" PRIu32 ",\"sum\":%" PRIu32 "}\n",
           output[0], output[1], scratch, sum);
    return 0;
}
