#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "strings/accumulator/wasm/dword.rs"]
mod cpu_parallel_accumulator_string_wasm;

#[path = "flags/stack/wasm/dword.rs"]
mod cpu_parallel_bit_flags_wasm;

#[path = "strings/bits/wasm/dword.rs"]
mod cpu_parallel_bit_string_wasm;

#[path = "integration/wasm/memory.rs"]
mod cpu_parallel_memory_wasm;

#[path = "lookup/wasm/products.rs"]
mod cpu_parallel_multiply_count_xlat_wasm;

#[path = "integration/control/wasm/reads.rs"]
mod cpu_parallel_reads_control_wasm;

#[path = "integration/wasm/core.rs"]
mod cpu_parallel_wasm;

#[path = "word/wasm/accumulator.rs"]
mod cpu_parallel_word_accumulator_wasm;

#[path = "word/wasm/arithmetic.rs"]
mod cpu_parallel_word_arithmetic_wasm;

#[path = "word/wasm/carry.rs"]
mod cpu_parallel_word_carry_wasm;

#[path = "word/wasm/logical.rs"]
mod cpu_parallel_word_logical_wasm;

#[path = "strings/compare/wasm/word.rs"]
mod cpu_parallel_word_move_compare_wasm;

#[path = "word/wasm/predicate.rs"]
mod cpu_parallel_word_predicate_wasm;

#[path = "word/wasm/register.rs"]
mod cpu_parallel_word_register_wasm;

#[path = "word/wasm/rotate.rs"]
mod cpu_parallel_word_rotate_wasm;

#[path = "word/wasm/shift.rs"]
mod cpu_parallel_word_shift_wasm;

#[path = "word/wasm/unary.rs"]
mod cpu_parallel_word_unary_wasm;

#[path = "integration/wasm/word.rs"]
mod cpu_parallel_word_wasm;

#[path = "word/wasm/admission.rs"]
mod word_batches_wasm;
