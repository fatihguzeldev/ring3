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
    uint32_t count, seed;
    if (argc != 3 || !decimal(argv[1], 256, &count) ||
        !decimal(argv[2], UINT32_MAX, &seed)) return 2;
    uint32_t values[256];
    for (uint32_t index = 0; index < count; ++index)
        values[index] = (seed ^ (UINT32_C(0x9e3779b9) * (index + 1))) + index;
    uint32_t sum = 0, last = 0, remaining = count;
    for (uint32_t index = 0; remaining != 0; ++index, --remaining) {
        last = values[index];
        sum += last;
    }
    printf("{\"sum\":%" PRIu32 ",\"last\":%" PRIu32
           ",\"remaining\":%" PRIu32 ",\"retired\":%" PRIu32 "}\n",
           sum, last, remaining, 7 * count + 4);
    return 0;
}
