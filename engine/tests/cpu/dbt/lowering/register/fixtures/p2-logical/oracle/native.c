#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static int decimal(const char *text, uint32_t limit, uint32_t *output) {
    if (*text == '\0') return 0;
    uint32_t value = 0;
    for (const char *cursor = text; *cursor != '\0'; ++cursor) {
        if (*cursor < '0' || *cursor > '9') return 0;
        const uint32_t digit = (uint32_t)(*cursor - '0');
        if (value > (limit - digit) / 10u) return 0;
        value = value * 10u + digit;
    }
    *output = value;
    return 1;
}

int main(int argc, char **argv) {
    uint32_t count, mask;
    if (argc != 3 || !decimal(argv[1], 10000u, &count) ||
        !decimal(argv[2], UINT32_MAX, &mask)) {
        fputs("usage: logical_oracle <decimal n 0..10000> <decimal u32 mask>\n", stderr);
        return 2;
    }
    uint32_t sum = 0, remaining = count, last = 0;
    while (remaining != 0) {
        last = ((remaining & 255u) | 256u) ^ mask;
        sum += last;
        --remaining;
    }
    /* Retirement is an authored fixture model, not native hardware counting. */
    printf("{\"sum\":%" PRIu32 ",\"remaining\":%" PRIu32
           ",\"last\":%" PRIu32 ",\"retired\":%" PRIu32 "}\n",
           sum, remaining, last, count * 9u + 3u);
    return 0;
}
