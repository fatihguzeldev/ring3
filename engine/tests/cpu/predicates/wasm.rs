#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "conditional_move/register/wasm/dword.rs"]
mod cpu_conditional_move_wasm;

#[path = "compare_test/memory/wasm/byte.rs"]
mod cpu_memory_byte_predicate_wasm;

#[path = "conditional_move/memory/wasm/dword.rs"]
mod cpu_memory_conditional_move_wasm;

#[path = "set/memory/wasm/byte.rs"]
mod cpu_memory_set_byte_wasm;

#[path = "set/register/wasm/byte.rs"]
mod cpu_set_byte_wasm;

#[path = "conditional_move/register/wasm/word.rs"]
mod cpu_word_conditional_move_wasm;

#[path = "conditional_move/memory/wasm/word.rs"]
mod cpu_word_memory_conditional_move_wasm;

#[path = "compare_test/memory/wasm/word.rs"]
mod cpu_word_memory_predicate_wasm;
