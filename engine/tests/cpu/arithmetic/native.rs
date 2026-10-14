#[path = "../../support/cpu/arena.rs"]
mod support_arena;

#[path = "../../support/cpu/prefix/byte.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "binary/register/native/byte.rs"]
mod cpu_byte_arithmetic;

#[path = "carry/register/native/byte.rs"]
mod cpu_byte_carry;

#[path = "carry/register/native/dword.rs"]
mod cpu_carry;

#[path = "adjustment/native/decimal.rs"]
mod cpu_decimal_adjust;

#[path = "binary/modify/native/byte.rs"]
mod cpu_memory_byte_arithmetic;

#[path = "carry/modify/native/byte.rs"]
mod cpu_memory_byte_carry;

#[path = "binary/read/native/byte.rs"]
mod cpu_register_memory_byte_arithmetic;

#[path = "carry/read/native/byte.rs"]
mod cpu_register_memory_byte_carry;

#[path = "carry/register/adc/word/native.rs"]
mod cpu_word_adc;

#[path = "binary/register/add/word/native.rs"]
mod cpu_word_add;

#[path = "binary/read/native/word.rs"]
mod cpu_word_memory_arithmetic;

#[path = "binary/modify/register/word/native.rs"]
mod cpu_word_memory_arithmetic_store;

#[path = "binary/inputs/native/store.rs"]
mod cpu_word_memory_arithmetic_store_legacy;

#[path = "carry/read/native/word.rs"]
mod cpu_word_memory_carry;

#[path = "carry/modify/register/word/native.rs"]
mod cpu_word_memory_carry_store;

#[path = "binary/modify/immediate/word/native.rs"]
mod cpu_word_memory_immediate_arithmetic_store;

#[path = "carry/register/sbb/word/native.rs"]
mod cpu_word_sbb;

#[path = "binary/register/subtract/word/native.rs"]
mod cpu_word_sub;

#[path = "aliases/native.rs"]
mod cpu_legacy_aliases;
