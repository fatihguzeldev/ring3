#[path = "../../support/loader/pe32/image.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "adjustment/wasm/ascii.rs"]
mod cpu_ascii_adjust_wasm;

#[path = "binary/register/wasm/byte.rs"]
mod cpu_byte_arithmetic_wasm;

#[path = "carry/register/wasm/byte.rs"]
mod cpu_byte_carry_wasm;

#[path = "carry/register/wasm/dword.rs"]
mod cpu_carry_wasm;

#[path = "adjustment/wasm/decimal.rs"]
mod cpu_decimal_adjust_wasm;

#[path = "binary/modify/wasm/byte.rs"]
mod cpu_memory_byte_arithmetic_wasm;

#[path = "carry/modify/wasm/byte.rs"]
mod cpu_memory_byte_carry_wasm;

#[path = "binary/read/wasm/byte.rs"]
mod cpu_register_memory_byte_arithmetic_wasm;

#[path = "carry/read/wasm/byte.rs"]
mod cpu_register_memory_byte_carry_wasm;

#[path = "carry/register/adc/word/wasm.rs"]
mod cpu_word_adc_wasm;

#[path = "binary/register/add/word/wasm.rs"]
mod cpu_word_add_wasm;

#[path = "binary/read/wasm/word.rs"]
mod cpu_word_memory_arithmetic_wasm;

#[path = "carry/modify/register/word/wasm.rs"]
mod cpu_word_memory_carry_store_wasm;

#[path = "carry/read/wasm/word.rs"]
mod cpu_word_memory_carry_wasm;

#[path = "binary/modify/immediate/word/wasm.rs"]
mod cpu_word_memory_immediate_arithmetic_store_wasm;

#[path = "carry/register/sbb/word/wasm.rs"]
mod cpu_word_sbb_wasm;

#[path = "binary/register/subtract/word/wasm.rs"]
mod cpu_word_sub_wasm;
