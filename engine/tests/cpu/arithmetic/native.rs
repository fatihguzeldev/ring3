#[path = "../../support/cpu/byte_prefix_admission.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
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

#[path = "carry/register/native/adc_word.rs"]
mod cpu_word_adc;

#[path = "binary/register/native/add_word.rs"]
mod cpu_word_add;

#[path = "binary/read/native/word.rs"]
mod cpu_word_memory_arithmetic;

#[path = "binary/modify/native/word_register.rs"]
mod cpu_word_memory_arithmetic_store;

#[path = "binary/inputs/native/store_banks.rs"]
mod cpu_word_memory_arithmetic_store_legacy;

#[path = "carry/read/native/word.rs"]
mod cpu_word_memory_carry;

#[path = "carry/modify/native/word_register.rs"]
mod cpu_word_memory_carry_store;

#[path = "binary/modify/native/word_immediate.rs"]
mod cpu_word_memory_immediate_arithmetic_store;

#[path = "carry/register/native/sbb_word.rs"]
mod cpu_word_sbb;

#[path = "binary/register/native/sub_word.rs"]
mod cpu_word_sub;
