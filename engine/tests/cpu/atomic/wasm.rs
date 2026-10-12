#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "compare_exchange/register/wasm/dword.rs"]
mod cpu_cmpxchg32_wasm;

#[path = "compare_exchange/register/wasm/byte.rs"]
mod cpu_cmpxchg8_wasm;

#[path = "logical/locked/wasm/and32.rs"]
mod cpu_locked_and32_wasm;

#[path = "bit_test/locked/wasm/reset32.rs"]
mod cpu_locked_btr32_wasm;

#[path = "bit_test/locked/wasm/set32.rs"]
mod cpu_locked_bts32_wasm;

#[path = "compare_exchange/locked/wasm/word.rs"]
mod cpu_locked_cmpxchg16_wasm;

#[path = "compare_exchange/locked/wasm/dword.rs"]
mod cpu_locked_cmpxchg32_wasm;

#[path = "compare_exchange/locked/wasm/byte.rs"]
mod cpu_locked_cmpxchg8_wasm;

#[path = "logical/flags/wasm/dynamic32.rs"]
mod cpu_locked_dynamic_flags32_wasm;

#[path = "logical/flags/wasm/full_mask32.rs"]
mod cpu_locked_fullmask32_wasm;

#[path = "logical/locked/wasm/or32.rs"]
mod cpu_locked_or32_wasm;

#[path = "fetch_add/locked/wasm/word.rs"]
mod cpu_locked_xadd16_wasm;

#[path = "fetch_add/locked/wasm/dword.rs"]
mod cpu_locked_xadd32_wasm;

#[path = "fetch_add/locked/wasm/byte.rs"]
mod cpu_locked_xadd8_wasm;

#[path = "logical/locked/wasm/xor32.rs"]
mod cpu_locked_xor32_wasm;

#[path = "bit_test/memory/wasm/mutation32.rs"]
mod cpu_memory_bit_mutation32_wasm;

#[path = "compare_exchange/memory/wasm/dword.rs"]
mod cpu_memory_cmpxchg32_wasm;

#[path = "compare_exchange/memory/wasm/byte.rs"]
mod cpu_memory_cmpxchg8_wasm;

#[path = "fetch_add/memory/wasm/dword.rs"]
mod cpu_memory_xadd32_wasm;

#[path = "fetch_add/memory/wasm/byte.rs"]
mod cpu_memory_xadd8_wasm;

#[path = "exchange/memory/wasm/word.rs"]
mod cpu_memory_xchg16_wasm;

#[path = "exchange/memory/wasm/dword.rs"]
mod cpu_memory_xchg32_wasm;

#[path = "exchange/memory/wasm/byte.rs"]
mod cpu_memory_xchg8_wasm;

#[path = "compare_exchange/register/wasm/word.rs"]
mod cpu_word_cmpxchg_wasm;

#[path = "compare_exchange/memory/wasm/word.rs"]
mod cpu_word_memory_cmpxchg_wasm;

#[path = "fetch_add/memory/wasm/word.rs"]
mod cpu_word_memory_xadd_wasm;

#[path = "fetch_add/register/wasm/word.rs"]
mod cpu_word_xadd_wasm;

#[path = "fetch_add/register/wasm/dword.rs"]
mod cpu_xadd32_wasm;

#[path = "fetch_add/register/wasm/byte.rs"]
mod cpu_xadd8_wasm;

#[path = "exchange/register/wasm/dword.rs"]
mod cpu_xchg32_wasm;

#[path = "exchange/register/wasm/byte.rs"]
mod cpu_xchg8_wasm;
