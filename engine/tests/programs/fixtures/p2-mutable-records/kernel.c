typedef signed char i8;
typedef unsigned char u8;
typedef signed short i16;
typedef unsigned short u16;
typedef signed int i32;
typedef unsigned int u32;

typedef struct Record8B {
    i8 signed_byte;
    u8 tag;
    i16 signed_word;
    u16 weight;
    u16 marker;
} Record8B;

_Static_assert(__CHAR_BIT__ == 8, "byte bits");
_Static_assert(sizeof(i8) == 1 && sizeof(u8) == 1, "byte widths");
_Static_assert(sizeof(i16) == 2 && sizeof(u16) == 2, "word widths");
_Static_assert(sizeof(i32) == 4 && sizeof(u32) == 4, "dword widths");
_Static_assert(sizeof(Record8B) == 8, "record width");
_Static_assert(__builtin_offsetof(Record8B, signed_byte) == 0, "signed byte offset");
_Static_assert(__builtin_offsetof(Record8B, tag) == 1, "tag offset");
_Static_assert(__builtin_offsetof(Record8B, signed_word) == 2, "signed word offset");
_Static_assert(__builtin_offsetof(Record8B, weight) == 4, "weight offset");
_Static_assert(__builtin_offsetof(Record8B, marker) == 6, "marker offset");

u32 ring3_reduce(Record8B *records, u32 count, u32 seed) {
    u32 acc = seed;
    for (u32 i = 0; i < count; ++i) {
        Record8B *record = &records[i];
        acc = (acc << 5) | (acc >> 27);
        u8 new_tag = (u8)(record->tag ^ (u8)acc);
        u16 new_weight = (u16)(record->weight + (u16)(acc >> 16));
        record->tag = new_tag;
        record->weight = new_weight;
        /* signed byte times signed word always fits i32. */
        i32 product = (i32)record->signed_byte * (i32)record->signed_word;
        if ((new_tag & 1u) != 0) {
            acc ^= (u32)product;
        } else {
            acc += (u32)product;
        }
        acc += (u32)new_weight * (u32)new_tag;
    }
    return acc;
}
