#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "carry_rotate/register/byte/wasm/cl.rs"]
mod cpu_byte_carry_rotate_cl_wasm;

#[path = "carry_rotate/register/byte/wasm/explicit_one.rs"]
mod cpu_byte_carry_rotate_immediate_one_wasm;

#[path = "carry_rotate/register/byte/wasm/immediate.rs"]
mod cpu_byte_carry_rotate_immediate_wasm;

#[path = "carry_rotate/register/byte/wasm/implicit_one.rs"]
mod cpu_byte_carry_rotate_one_wasm;

#[path = "rotate/register/byte/wasm/cl.rs"]
mod cpu_byte_rotate_cl_wasm;

#[path = "rotate/register/byte/wasm/explicit_one.rs"]
mod cpu_byte_rotate_immediate_one_wasm;

#[path = "rotate/register/byte/wasm/immediate.rs"]
mod cpu_byte_rotate_immediate_wasm;

#[path = "rotate/register/byte/wasm/implicit_one.rs"]
mod cpu_byte_rotate_one_wasm;

#[path = "single/register/wasm/byte.rs"]
mod cpu_byte_shift_wasm;

#[path = "carry_rotate/register/dword/wasm/cl.rs"]
mod cpu_carry_rotate_cl_wasm;

#[path = "carry_rotate/register/dword/wasm/explicit_one.rs"]
mod cpu_carry_rotate_immediate_one_wasm;

#[path = "carry_rotate/register/dword/wasm/immediate.rs"]
mod cpu_carry_rotate_immediate_wasm;

#[path = "carry_rotate/register/dword/wasm/implicit_one.rs"]
mod cpu_carry_rotate_one_wasm;

#[path = "double/register/wasm/dword.rs"]
mod cpu_double_shift32_wasm;

#[path = "carry_rotate/memory/byte/wasm/immediate.rs"]
mod cpu_memory_byte_carry_rotate_immediate_wasm;

#[path = "carry_rotate/memory/byte/wasm/implicit_one.rs"]
mod cpu_memory_byte_carry_rotate_one_wasm;

#[path = "rotate/memory/byte/wasm/implicit_one.rs"]
mod cpu_memory_byte_rotate_one_wasm;

#[path = "rotate/memory/byte/wasm/counts.rs"]
mod cpu_memory_byte_rotate_wasm;

#[path = "single/memory/byte/wasm/cl.rs"]
mod cpu_memory_byte_shift_cl_wasm;

#[path = "single/memory/byte/wasm/explicit_one.rs"]
mod cpu_memory_byte_shift_immediate_one_wasm;

#[path = "single/memory/byte/wasm/immediate.rs"]
mod cpu_memory_byte_shift_immediate_wasm;

#[path = "single/memory/byte/wasm/implicit_one.rs"]
mod cpu_memory_byte_shift_wasm;

#[path = "carry_rotate/memory/dword/wasm/cl.rs"]
mod cpu_memory_carry_rotate_cl_wasm;

#[path = "carry_rotate/memory/dword/wasm/immediate.rs"]
mod cpu_memory_carry_rotate_immediate_wasm;

#[path = "carry_rotate/memory/dword/wasm/implicit_one.rs"]
mod cpu_memory_carry_rotate_one_wasm;

#[path = "double/memory/wasm/dword.rs"]
mod cpu_memory_double_shift32_wasm;

#[path = "rotate/memory/dword/wasm/counts.rs"]
mod cpu_memory_rotate32_wasm;

#[path = "rotate/memory/dword/wasm/implicit_one.rs"]
mod cpu_memory_rotate_one_wasm;

#[path = "single/memory/wasm/dword.rs"]
mod cpu_memory_shift_wasm;

#[path = "rotate/register/dword/wasm/counts.rs"]
mod cpu_rotate32_wasm;

#[path = "rotate/register/dword/wasm/explicit_one.rs"]
mod cpu_rotate_immediate_one_wasm;

#[path = "rotate/register/dword/wasm/implicit_one.rs"]
mod cpu_rotate_one_wasm;

#[path = "single/register/wasm/dword.rs"]
mod cpu_shift_wasm;

#[path = "carry_rotate/register/wasm/word.rs"]
mod cpu_word_carry_rotate_wasm;

#[path = "double/register/wasm/word.rs"]
mod cpu_word_double_shift_wasm;

#[path = "single/register/wasm/logical_word.rs"]
mod cpu_word_logical_shift_wasm;

#[path = "carry_rotate/memory/wasm/word.rs"]
mod cpu_word_memory_carry_rotate_wasm;

#[path = "double/memory/wasm/word.rs"]
mod cpu_word_memory_double_shift_wasm;

#[path = "rotate/memory/wasm/word.rs"]
mod cpu_word_memory_rotate_wasm;

#[path = "single/memory/wasm/word.rs"]
mod cpu_word_memory_shift_wasm;

#[path = "rotate/register/wasm/rol_word.rs"]
mod cpu_word_rol_wasm;

#[path = "rotate/register/wasm/ror_word.rs"]
mod cpu_word_ror_wasm;

#[path = "single/register/wasm/arithmetic_word.rs"]
mod cpu_word_sar_wasm;

#[path = "carry_rotate/memory/byte/wasm/cl.rs"]
mod wasm_memory_byte_carry_rotate_cl;
