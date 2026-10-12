#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "accumulator/register/wasm/dword.rs"]
mod cpu_accumulator_multiply_wasm;

#[path = "accumulator/register/wasm/byte.rs"]
mod cpu_byte_accumulator_multiply_wasm;

#[path = "divide/register/wasm/byte.rs"]
mod cpu_byte_division_wasm;

#[path = "divide/register/wasm/dword.rs"]
mod cpu_division_wasm;

#[path = "accumulator/memory/wasm/dword.rs"]
mod cpu_memory_accumulator_multiply_wasm;

#[path = "accumulator/memory/wasm/byte.rs"]
mod cpu_memory_byte_accumulator_multiply_wasm;

#[path = "divide/memory/wasm/byte.rs"]
mod cpu_memory_byte_division_wasm;

#[path = "divide/memory/wasm/dword.rs"]
mod cpu_memory_division_wasm;

#[path = "signed/memory/wasm/dword.rs"]
mod cpu_memory_multiply_wasm;

#[path = "signed/register/wasm/dword.rs"]
mod cpu_multiply_wasm;

#[path = "accumulator/register/wasm/word.rs"]
mod cpu_word_accumulator_multiply_wasm;

#[path = "divide/register/wasm/word.rs"]
mod cpu_word_division_wasm;

#[path = "signed/immediate/wasm/word.rs"]
mod cpu_word_immediate_multiply_wasm;

#[path = "signed/register/wasm/word.rs"]
mod cpu_word_multiply_wasm;
