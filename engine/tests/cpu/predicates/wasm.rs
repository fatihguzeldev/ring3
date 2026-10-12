#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/cpu_conditional_move_wasm.rs"]
mod cpu_conditional_move_wasm;

#[path = "cases/wasm/cpu_memory_byte_predicate_wasm.rs"]
mod cpu_memory_byte_predicate_wasm;

#[path = "cases/wasm/cpu_memory_conditional_move_wasm.rs"]
mod cpu_memory_conditional_move_wasm;

#[path = "cases/wasm/cpu_memory_set_byte_wasm.rs"]
mod cpu_memory_set_byte_wasm;

#[path = "cases/wasm/cpu_set_byte_wasm.rs"]
mod cpu_set_byte_wasm;

#[path = "cases/wasm/cpu_word_conditional_move_wasm.rs"]
mod cpu_word_conditional_move_wasm;

#[path = "cases/wasm/cpu_word_memory_conditional_move_wasm.rs"]
mod cpu_word_memory_conditional_move_wasm;

#[path = "cases/wasm/cpu_word_memory_predicate_wasm.rs"]
mod cpu_word_memory_predicate_wasm;
