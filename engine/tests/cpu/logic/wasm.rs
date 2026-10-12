#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "compare/register/wasm/byte.rs"]
mod cpu_byte_compare_wasm;

#[path = "binary/register/wasm/byte.rs"]
mod cpu_byte_logical_wasm;

#[path = "test/register/wasm/byte.rs"]
mod cpu_byte_test_wasm;

#[path = "unary/register/wasm/byte.rs"]
mod cpu_byte_unary_wasm;

#[path = "binary/modify/wasm/byte.rs"]
mod cpu_memory_byte_logical_wasm;

#[path = "unary/memory/wasm/byte.rs"]
mod cpu_memory_byte_unary_wasm;

#[path = "compare/read/wasm/byte.rs"]
mod cpu_register_memory_byte_compare_wasm;

#[path = "binary/read/wasm/byte.rs"]
mod cpu_register_memory_byte_logical_wasm;

#[path = "binary/register/and/word/wasm.rs"]
mod cpu_word_and_wasm;

#[path = "compare/register/wasm/word.rs"]
mod cpu_word_compare_wasm;

#[path = "unary/register/inc_dec/word/wasm.rs"]
mod cpu_word_inc_dec_wasm;

#[path = "binary/modify/immediate/word/wasm.rs"]
mod cpu_word_memory_immediate_logical_store_wasm;

#[path = "unary/memory/wasm/word.rs"]
mod cpu_word_memory_unary_wasm;

#[path = "unary/register/neg_not/word/wasm.rs"]
mod cpu_word_neg_not_wasm;

#[path = "binary/register/or_xor/word/wasm.rs"]
mod cpu_word_or_xor_wasm;

#[path = "test/register/wasm/word.rs"]
mod cpu_word_test_wasm;
