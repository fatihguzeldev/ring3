#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "carry_rotate/register/byte/wasm/cl.rs"]
mod cpu_byte_carry_rotate_cl_wasm;

#[path = "carry_rotate/immediate/one/byte/wasm.rs"]
mod cpu_byte_carry_rotate_immediate_one_wasm;

#[path = "carry_rotate/register/byte/wasm/immediate.rs"]
mod cpu_byte_carry_rotate_immediate_wasm;

#[path = "carry_rotate/register/one/byte/wasm.rs"]
mod cpu_byte_carry_rotate_one_wasm;

#[path = "rotate/register/byte/wasm/cl.rs"]
mod cpu_byte_rotate_cl_wasm;

#[path = "rotate/immediate/one/byte/wasm.rs"]
mod cpu_byte_rotate_immediate_one_wasm;

#[path = "rotate/register/byte/wasm/immediate.rs"]
mod cpu_byte_rotate_immediate_wasm;

#[path = "rotate/register/one/byte/wasm.rs"]
mod cpu_byte_rotate_one_wasm;

#[path = "single/register/wasm/byte.rs"]
mod cpu_byte_shift_wasm;

#[path = "carry_rotate/register/dword/wasm/cl.rs"]
mod cpu_carry_rotate_cl_wasm;

#[path = "carry_rotate/immediate/one/dword/wasm.rs"]
mod cpu_carry_rotate_immediate_one_wasm;

#[path = "carry_rotate/register/dword/wasm/immediate.rs"]
mod cpu_carry_rotate_immediate_wasm;

#[path = "carry_rotate/register/one/dword/wasm.rs"]
mod cpu_carry_rotate_one_wasm;

#[path = "double/register/wasm/dword.rs"]
mod cpu_double_shift32_wasm;

#[path = "carry_rotate/memory/byte/wasm/immediate.rs"]
mod cpu_memory_byte_carry_rotate_immediate_wasm;

#[path = "carry_rotate/memory/one/byte/wasm.rs"]
mod cpu_memory_byte_carry_rotate_one_wasm;

#[path = "rotate/memory/one/byte/wasm.rs"]
mod cpu_memory_byte_rotate_one_wasm;

#[path = "rotate/memory/byte/wasm/counts.rs"]
mod cpu_memory_byte_rotate_wasm;

#[path = "single/memory/byte/wasm/cl.rs"]
mod cpu_memory_byte_shift_cl_wasm;

#[path = "single/memory/immediate/one/byte/wasm.rs"]
mod cpu_memory_byte_shift_immediate_one_wasm;

#[path = "single/memory/byte/wasm/immediate.rs"]
mod cpu_memory_byte_shift_immediate_wasm;

#[path = "single/memory/one/byte/wasm.rs"]
mod cpu_memory_byte_shift_wasm;

#[path = "carry_rotate/memory/dword/wasm/cl.rs"]
mod cpu_memory_carry_rotate_cl_wasm;

#[path = "carry_rotate/memory/dword/wasm/immediate.rs"]
mod cpu_memory_carry_rotate_immediate_wasm;

#[path = "carry_rotate/memory/one/dword/wasm.rs"]
mod cpu_memory_carry_rotate_one_wasm;

#[path = "double/memory/wasm/dword.rs"]
mod cpu_memory_double_shift32_wasm;

#[path = "rotate/memory/dword/wasm/counts.rs"]
mod cpu_memory_rotate32_wasm;

#[path = "rotate/memory/one/dword/wasm.rs"]
mod cpu_memory_rotate_one_wasm;

#[path = "single/memory/wasm/dword.rs"]
mod cpu_memory_shift_wasm;

#[path = "rotate/register/dword/wasm/counts.rs"]
mod cpu_rotate32_wasm;

#[path = "rotate/immediate/one/dword/wasm.rs"]
mod cpu_rotate_immediate_one_wasm;

#[path = "rotate/register/one/dword/wasm.rs"]
mod cpu_rotate_one_wasm;

#[path = "single/register/wasm/dword.rs"]
mod cpu_shift_wasm;

#[path = "carry_rotate/register/wasm/word.rs"]
mod cpu_word_carry_rotate_wasm;

#[path = "double/register/wasm/word.rs"]
mod cpu_word_double_shift_wasm;

#[path = "single/register/word/wasm/logical.rs"]
mod cpu_word_logical_shift_wasm;

#[path = "carry_rotate/memory/wasm/word.rs"]
mod cpu_word_memory_carry_rotate_wasm;

#[path = "double/memory/wasm/word.rs"]
mod cpu_word_memory_double_shift_wasm;

#[path = "rotate/memory/wasm/word.rs"]
mod cpu_word_memory_rotate_wasm;

#[path = "single/memory/wasm/word.rs"]
mod cpu_word_memory_shift_wasm;

#[path = "rotate/register/word/wasm/left.rs"]
mod cpu_word_rol_wasm;

#[path = "rotate/register/word/wasm/right.rs"]
mod cpu_word_ror_wasm;

#[path = "single/register/word/wasm/arithmetic.rs"]
mod cpu_word_sar_wasm;

#[path = "carry_rotate/memory/byte/wasm/cl.rs"]
mod wasm_memory_byte_carry_rotate_cl;
