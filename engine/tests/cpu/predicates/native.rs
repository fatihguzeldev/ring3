#[path = "../../support/cpu/admission/banks.rs"]
mod support_memory_banks;

#[path = "../../support/cpu/admission/native.rs"]
mod support_admission;

#[path = "../../support/cpu/wasm/native.rs"]
mod support_wasm_interface;

#[path = "../../support/cpu/fetch/native.rs"]
mod support_fetch;

#[path = "../../support/cpu/prefix/byte.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "conditional_move/register/native/dword.rs"]
mod cpu_conditional_move;

#[path = "compare_test/memory/native/byte.rs"]
mod cpu_memory_byte_predicate;

#[path = "conditional_move/memory/native/dword.rs"]
mod cpu_memory_conditional_move;

#[path = "set/memory/native/byte.rs"]
mod cpu_memory_set_byte;

#[path = "set/register/native/byte.rs"]
mod cpu_set_byte;

#[path = "conditional_move/register/native/word.rs"]
mod cpu_word_conditional_move;

#[path = "conditional_move/memory/native/word.rs"]
mod cpu_word_memory_conditional_move;

#[path = "compare_test/memory/native/word.rs"]
mod cpu_word_memory_predicate;
