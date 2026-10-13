#[path = "../../support/cpu/arena.rs"]
mod support_arena;

#[path = "../../support/cpu/prefix/byte.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "accumulator/register/native/dword.rs"]
mod cpu_accumulator_multiply;

#[path = "accumulator/register/native/byte.rs"]
mod cpu_byte_accumulator_multiply;

#[path = "divide/register/native/byte.rs"]
mod cpu_byte_division;

#[path = "divide/register/native/dword.rs"]
mod cpu_division;

#[path = "accumulator/memory/native/dword.rs"]
mod cpu_memory_accumulator_multiply;

#[path = "accumulator/memory/native/byte.rs"]
mod cpu_memory_byte_accumulator_multiply;

#[path = "divide/memory/native/byte.rs"]
mod cpu_memory_byte_division;

#[path = "divide/memory/native/dword.rs"]
mod cpu_memory_division;

#[path = "signed/memory/native/dword.rs"]
mod cpu_memory_multiply;

#[path = "signed/register/native/dword.rs"]
mod cpu_multiply;

#[path = "accumulator/register/native/word.rs"]
mod cpu_word_accumulator_multiply;

#[path = "divide/register/native/word.rs"]
mod cpu_word_division;

#[path = "signed/immediate/native/word.rs"]
mod cpu_word_immediate_multiply;

#[path = "signed/register/native/word.rs"]
mod cpu_word_multiply;
