#[path = "../../support/cpu/byte_prefix_admission.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
mod support_word_prefix;

#[path = "carry_rotate/register/byte/native/cl.rs"]
mod cpu_byte_carry_rotate_cl;

#[path = "carry_rotate/register/byte/native/immediate.rs"]
mod cpu_byte_carry_rotate_immediate;

#[path = "carry_rotate/register/byte/native/explicit_one.rs"]
mod cpu_byte_carry_rotate_immediate_one;

#[path = "carry_rotate/register/byte/native/implicit_one.rs"]
mod cpu_byte_carry_rotate_one;

#[path = "rotate/register/byte/native/cl.rs"]
mod cpu_byte_rotate_cl;

#[path = "rotate/register/byte/native/immediate.rs"]
mod cpu_byte_rotate_immediate;

#[path = "rotate/register/byte/native/explicit_one.rs"]
mod cpu_byte_rotate_immediate_one;

#[path = "rotate/register/byte/native/implicit_one.rs"]
mod cpu_byte_rotate_one;

#[path = "single/register/native/byte.rs"]
mod cpu_byte_shift;

#[path = "carry_rotate/register/dword/native/cl.rs"]
mod cpu_carry_rotate_cl32;

#[path = "carry_rotate/register/dword/native/immediate.rs"]
mod cpu_carry_rotate_immediate32;

#[path = "carry_rotate/register/dword/native/explicit_one.rs"]
mod cpu_carry_rotate_immediate_one;

#[path = "carry_rotate/register/dword/native/implicit_one.rs"]
mod cpu_carry_rotate_one;

#[path = "double/register/native/dword.rs"]
mod cpu_double_shift32;

#[path = "carry_rotate/memory/byte/native/cl.rs"]
mod cpu_memory_byte_carry_rotate_cl;

#[path = "carry_rotate/memory/byte/native/immediate.rs"]
mod cpu_memory_byte_carry_rotate_immediate;

#[path = "carry_rotate/memory/byte/native/implicit_one.rs"]
mod cpu_memory_byte_carry_rotate_one;

#[path = "rotate/memory/byte/native/counts.rs"]
mod cpu_memory_byte_rotate;

#[path = "rotate/memory/byte/native/implicit_one.rs"]
mod cpu_memory_byte_rotate_one;

#[path = "single/memory/byte/native/implicit_one.rs"]
mod cpu_memory_byte_shift;

#[path = "single/memory/byte/native/cl.rs"]
mod cpu_memory_byte_shift_cl;

#[path = "single/memory/byte/native/immediate.rs"]
mod cpu_memory_byte_shift_immediate;

#[path = "single/memory/byte/native/explicit_one.rs"]
mod cpu_memory_byte_shift_immediate_one;

#[path = "carry_rotate/memory/dword/native/cl.rs"]
mod cpu_memory_carry_rotate_cl32;

#[path = "carry_rotate/memory/dword/native/immediate.rs"]
mod cpu_memory_carry_rotate_immediate32;

#[path = "carry_rotate/memory/dword/native/implicit_one.rs"]
mod cpu_memory_carry_rotate_one;

#[path = "double/memory/native/dword.rs"]
mod cpu_memory_double_shift32;

#[path = "rotate/memory/dword/native/counts.rs"]
mod cpu_memory_rotate32;

#[path = "rotate/memory/dword/native/implicit_one.rs"]
mod cpu_memory_rotate_one;

#[path = "single/memory/native/dword.rs"]
mod cpu_memory_shift;

#[path = "rotate/register/dword/native/counts.rs"]
mod cpu_rotate32;

#[path = "rotate/register/dword/native/explicit_one.rs"]
mod cpu_rotate_immediate_one;

#[path = "rotate/register/dword/native/implicit_one.rs"]
mod cpu_rotate_one;

#[path = "single/register/native/dword.rs"]
mod cpu_shift;

#[path = "carry_rotate/register/native/word.rs"]
mod cpu_word_carry_rotate;

#[path = "double/register/native/word.rs"]
mod cpu_word_double_shift;

#[path = "single/register/native/logical_word.rs"]
mod cpu_word_logical_shift;

#[path = "carry_rotate/memory/native/word.rs"]
mod cpu_word_memory_carry_rotate;

#[path = "double/memory/native/word.rs"]
mod cpu_word_memory_double_shift;

#[path = "rotate/memory/native/word.rs"]
mod cpu_word_memory_rotate;

#[path = "single/memory/native/word.rs"]
mod cpu_word_memory_shift;

#[path = "rotate/register/native/rol_word.rs"]
mod cpu_word_rol;

#[path = "rotate/register/native/ror_word.rs"]
mod cpu_word_ror;

#[path = "single/register/native/arithmetic_word.rs"]
mod cpu_word_sar;
