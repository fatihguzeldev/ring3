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

#[path = "carry_rotate/immediate/one/byte/native.rs"]
mod cpu_byte_carry_rotate_immediate_one;

#[path = "carry_rotate/register/one/byte/native.rs"]
mod cpu_byte_carry_rotate_one;

#[path = "rotate/register/byte/native/cl.rs"]
mod cpu_byte_rotate_cl;

#[path = "rotate/register/byte/native/immediate.rs"]
mod cpu_byte_rotate_immediate;

#[path = "rotate/immediate/one/byte/native.rs"]
mod cpu_byte_rotate_immediate_one;

#[path = "rotate/register/one/byte/native.rs"]
mod cpu_byte_rotate_one;

#[path = "single/register/native/byte.rs"]
mod cpu_byte_shift;

#[path = "carry_rotate/register/dword/native/cl.rs"]
mod cpu_carry_rotate_cl32;

#[path = "carry_rotate/register/dword/native/immediate.rs"]
mod cpu_carry_rotate_immediate32;

#[path = "carry_rotate/immediate/one/dword/native.rs"]
mod cpu_carry_rotate_immediate_one;

#[path = "carry_rotate/register/one/dword/native.rs"]
mod cpu_carry_rotate_one;

#[path = "double/register/native/dword.rs"]
mod cpu_double_shift32;

#[path = "carry_rotate/memory/byte/native/cl.rs"]
mod cpu_memory_byte_carry_rotate_cl;

#[path = "carry_rotate/memory/byte/native/immediate.rs"]
mod cpu_memory_byte_carry_rotate_immediate;

#[path = "carry_rotate/memory/one/byte/native.rs"]
mod cpu_memory_byte_carry_rotate_one;

#[path = "rotate/memory/byte/native/counts.rs"]
mod cpu_memory_byte_rotate;

#[path = "rotate/memory/one/byte/native.rs"]
mod cpu_memory_byte_rotate_one;

#[path = "single/memory/one/byte/native.rs"]
mod cpu_memory_byte_shift;

#[path = "single/memory/byte/native/cl.rs"]
mod cpu_memory_byte_shift_cl;

#[path = "single/memory/byte/native/immediate.rs"]
mod cpu_memory_byte_shift_immediate;

#[path = "single/memory/immediate/one/byte/native.rs"]
mod cpu_memory_byte_shift_immediate_one;

#[path = "carry_rotate/memory/dword/native/cl.rs"]
mod cpu_memory_carry_rotate_cl32;

#[path = "carry_rotate/memory/dword/native/immediate.rs"]
mod cpu_memory_carry_rotate_immediate32;

#[path = "carry_rotate/memory/one/dword/native.rs"]
mod cpu_memory_carry_rotate_one;

#[path = "double/memory/native/dword.rs"]
mod cpu_memory_double_shift32;

#[path = "rotate/memory/dword/native/counts.rs"]
mod cpu_memory_rotate32;

#[path = "rotate/memory/one/dword/native.rs"]
mod cpu_memory_rotate_one;

#[path = "single/memory/native/dword.rs"]
mod cpu_memory_shift;

#[path = "rotate/register/dword/native/counts.rs"]
mod cpu_rotate32;

#[path = "rotate/immediate/one/dword/native.rs"]
mod cpu_rotate_immediate_one;

#[path = "rotate/register/one/dword/native.rs"]
mod cpu_rotate_one;

#[path = "single/register/native/dword.rs"]
mod cpu_shift;

#[path = "carry_rotate/register/native/word.rs"]
mod cpu_word_carry_rotate;

#[path = "double/register/native/word.rs"]
mod cpu_word_double_shift;

#[path = "single/register/word/native/logical.rs"]
mod cpu_word_logical_shift;

#[path = "carry_rotate/memory/native/word.rs"]
mod cpu_word_memory_carry_rotate;

#[path = "double/memory/native/word.rs"]
mod cpu_word_memory_double_shift;

#[path = "rotate/memory/native/word.rs"]
mod cpu_word_memory_rotate;

#[path = "single/memory/native/word.rs"]
mod cpu_word_memory_shift;

#[path = "rotate/register/word/native/left.rs"]
mod cpu_word_rol;

#[path = "rotate/register/word/native/right.rs"]
mod cpu_word_ror;

#[path = "single/register/word/native/arithmetic.rs"]
mod cpu_word_sar;
