#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static int count_value(const char *text, uint32_t *out) {
    uint32_t value = 0;
    if (*text == '\0') return 0;
    for (; *text != '\0'; ++text) {
        if (*text < '0' || *text > '9') return 0;
        uint32_t digit = (uint32_t)(*text - '0');
        if (value > 2000 || (value == 2000 && digit > 0)) return 0;
        value = value * 10 + digit;
    }
    *out = value;
    return 1;
}

int main(int argc, char **argv) {
    uint32_t count;
    if (argc != 2 || !count_value(argv[1], &count)) return 2;
    uint32_t sum = 0, remaining = count;
    while (remaining != 0) sum += remaining--;
    printf("{\"sum\":%" PRIu32 ",\"remaining\":%" PRIu32
           ",\"retired\":%" PRIu32 "}\n", sum, remaining, 6 * count + 5);
    return 0;
}
