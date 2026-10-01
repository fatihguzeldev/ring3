#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

int main(int argc, char **argv) {
    if (argc != 2 || argv[1][0] == '\0') {
        fputs("usage: baseline_oracle <decimal n in 0..100000>\n", stderr);
        return 2;
    }

    uint32_t input = 0;
    for (const char *cursor = argv[1]; *cursor != '\0'; ++cursor) {
        if (*cursor < '0' || *cursor > '9') {
            fputs("invalid decimal input\n", stderr);
            return 2;
        }
        const uint32_t digit = (uint32_t)(*cursor - '0');
        if (input > (100000u - digit) / 10u) {
            fputs("input exceeds 100000\n", stderr);
            return 2;
        }
        input = input * 10u + digit;
    }

    uint32_t remaining = input;
    uint32_t sum = 0;
    while (remaining != 0) {
        sum += remaining;
        --remaining;
    }

    const uint32_t retired = input * 5u + 3u;
    printf("{\"sum\":%" PRIu32 ",\"remaining\":%" PRIu32
           ",\"retired\":%" PRIu32 "}\n", sum, remaining, retired);
    return 0;
}
