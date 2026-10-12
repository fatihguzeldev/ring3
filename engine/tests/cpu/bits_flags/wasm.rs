#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "integration/wasm/scalar.rs"]
mod cpu_scalar_bits_wasm;

#[path = "scan/register/wasm/dword.rs"]
mod cpu_bit_scan_wasm;

#[path = "test/register/wasm/dword.rs"]
mod cpu_bit_test32_wasm;

#[path = "byte_order/wasm/dword.rs"]
mod cpu_bswap32_wasm;

#[path = "flags/wasm/ah.rs"]
mod cpu_flags_ah_wasm;

#[path = "scan/memory/wasm/dword.rs"]
mod cpu_memory_bit_scan_wasm;

#[path = "test/memory/wasm/dword.rs"]
mod cpu_memory_bit_test32_wasm;

#[path = "scan/register/wasm/word.rs"]
mod cpu_word_bit_scan_wasm;

#[path = "test/register/wasm/word.rs"]
mod cpu_word_bit_test_wasm;

#[path = "test/memory/wasm/word.rs"]
mod cpu_word_memory_bit_test_wasm;

#[path = "manipulation/wasm/bmi1.rs"]
mod cpu_bmi1_wasm;

#[path = "manipulation/wasm/bmi2.rs"]
mod cpu_bmi2_wasm;
