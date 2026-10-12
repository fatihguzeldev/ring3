#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/cpu_ascii_adjust_wasm.rs"]
mod cpu_ascii_adjust_wasm;

#[path = "cases/wasm/cpu_byte_arithmetic_wasm.rs"]
mod cpu_byte_arithmetic_wasm;

#[path = "cases/wasm/cpu_byte_carry_wasm.rs"]
mod cpu_byte_carry_wasm;

#[path = "cases/wasm/cpu_carry_wasm.rs"]
mod cpu_carry_wasm;

#[path = "cases/wasm/cpu_decimal_adjust_wasm.rs"]
mod cpu_decimal_adjust_wasm;

#[path = "cases/wasm/cpu_memory_byte_arithmetic_wasm.rs"]
mod cpu_memory_byte_arithmetic_wasm;

#[path = "cases/wasm/cpu_memory_byte_carry_wasm.rs"]
mod cpu_memory_byte_carry_wasm;

#[path = "cases/wasm/cpu_register_memory_byte_arithmetic_wasm.rs"]
mod cpu_register_memory_byte_arithmetic_wasm;

#[path = "cases/wasm/cpu_register_memory_byte_carry_wasm.rs"]
mod cpu_register_memory_byte_carry_wasm;

#[path = "cases/wasm/cpu_word_adc_wasm.rs"]
mod cpu_word_adc_wasm;

#[path = "cases/wasm/cpu_word_add_wasm.rs"]
mod cpu_word_add_wasm;

#[path = "cases/wasm/cpu_word_memory_arithmetic_wasm.rs"]
mod cpu_word_memory_arithmetic_wasm;

#[path = "cases/wasm/cpu_word_memory_carry_store_wasm.rs"]
mod cpu_word_memory_carry_store_wasm;

#[path = "cases/wasm/cpu_word_memory_carry_wasm.rs"]
mod cpu_word_memory_carry_wasm;

#[path = "cases/wasm/cpu_word_memory_immediate_arithmetic_store_wasm.rs"]
mod cpu_word_memory_immediate_arithmetic_store_wasm;

#[path = "cases/wasm/cpu_word_sbb_wasm.rs"]
mod cpu_word_sbb_wasm;

#[path = "cases/wasm/cpu_word_sub_wasm.rs"]
mod cpu_word_sub_wasm;
