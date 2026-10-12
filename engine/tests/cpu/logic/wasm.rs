#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/cpu_byte_compare_wasm.rs"]
mod cpu_byte_compare_wasm;

#[path = "cases/wasm/cpu_byte_logical_wasm.rs"]
mod cpu_byte_logical_wasm;

#[path = "cases/wasm/cpu_byte_test_wasm.rs"]
mod cpu_byte_test_wasm;

#[path = "cases/wasm/cpu_byte_unary_wasm.rs"]
mod cpu_byte_unary_wasm;

#[path = "cases/wasm/cpu_memory_byte_logical_wasm.rs"]
mod cpu_memory_byte_logical_wasm;

#[path = "cases/wasm/cpu_memory_byte_unary_wasm.rs"]
mod cpu_memory_byte_unary_wasm;

#[path = "cases/wasm/cpu_register_memory_byte_compare_wasm.rs"]
mod cpu_register_memory_byte_compare_wasm;

#[path = "cases/wasm/cpu_register_memory_byte_logical_wasm.rs"]
mod cpu_register_memory_byte_logical_wasm;

#[path = "cases/wasm/cpu_word_and_wasm.rs"]
mod cpu_word_and_wasm;

#[path = "cases/wasm/cpu_word_compare_wasm.rs"]
mod cpu_word_compare_wasm;

#[path = "cases/wasm/cpu_word_inc_dec_wasm.rs"]
mod cpu_word_inc_dec_wasm;

#[path = "cases/wasm/cpu_word_memory_immediate_logical_store_wasm.rs"]
mod cpu_word_memory_immediate_logical_store_wasm;

#[path = "cases/wasm/cpu_word_memory_unary_wasm.rs"]
mod cpu_word_memory_unary_wasm;

#[path = "cases/wasm/cpu_word_neg_not_wasm.rs"]
mod cpu_word_neg_not_wasm;

#[path = "cases/wasm/cpu_word_or_xor_wasm.rs"]
mod cpu_word_or_xor_wasm;

#[path = "cases/wasm/cpu_word_test_wasm.rs"]
mod cpu_word_test_wasm;
