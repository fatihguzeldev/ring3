#[path = "../../support/cpu/byte_prefix_admission.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
mod support_word_prefix;

#[path = "compare_exchange/register/native/dword.rs"]
mod cpu_cmpxchg32;

#[path = "compare_exchange/register/native/byte.rs"]
mod cpu_cmpxchg8;

#[path = "exchange/prefixes/native/lock.rs"]
mod cpu_lock_exchange_prefixes;

#[path = "logical/locked/native/and32.rs"]
mod cpu_locked_and32;

#[path = "bit_test/locked/native/reset32.rs"]
mod cpu_locked_btr32;

#[path = "bit_test/locked/native/set32.rs"]
mod cpu_locked_bts32;

#[path = "compare_exchange/locked/native/word.rs"]
mod cpu_locked_cmpxchg16;

#[path = "compare_exchange/locked/native/dword.rs"]
mod cpu_locked_cmpxchg32;

#[path = "compare_exchange/locked/native/byte.rs"]
mod cpu_locked_cmpxchg8;

#[path = "logical/locked/native/immediate32.rs"]
mod cpu_locked_logical_imm32;

#[path = "logical/locked/native/register32.rs"]
mod cpu_locked_logical_register32;

#[path = "logical/locked/native/or32.rs"]
mod cpu_locked_or32;

#[path = "fetch_add/locked/native/word.rs"]
mod cpu_locked_xadd16;

#[path = "fetch_add/locked/native/dword.rs"]
mod cpu_locked_xadd32;

#[path = "fetch_add/locked/native/byte.rs"]
mod cpu_locked_xadd8;

#[path = "logical/locked/native/xor32.rs"]
mod cpu_locked_xor32;

#[path = "bit_test/memory/native/mutation32.rs"]
mod cpu_memory_bit_mutation32;

#[path = "../dbt/lowering/memory/native/bridge.rs"]
mod cpu_memory_bit_mutation_bridge;

#[path = "compare_exchange/memory/native/dword.rs"]
mod cpu_memory_cmpxchg32;

#[path = "compare_exchange/memory/native/byte.rs"]
mod cpu_memory_cmpxchg8;

#[path = "fetch_add/memory/native/dword.rs"]
mod cpu_memory_xadd32;

#[path = "fetch_add/memory/native/byte.rs"]
mod cpu_memory_xadd8;

#[path = "exchange/memory/native/word.rs"]
mod cpu_memory_xchg16;

#[path = "exchange/memory/native/dword.rs"]
mod cpu_memory_xchg32;

#[path = "exchange/memory/native/byte.rs"]
mod cpu_memory_xchg8;

#[path = "compare_exchange/register/native/word.rs"]
mod cpu_word_cmpxchg;

#[path = "compare_exchange/memory/native/word.rs"]
mod cpu_word_memory_cmpxchg;

#[path = "fetch_add/memory/native/word.rs"]
mod cpu_word_memory_xadd;

#[path = "fetch_add/register/native/word.rs"]
mod cpu_word_xadd;

#[path = "fetch_add/register/native/dword.rs"]
mod cpu_xadd32;

#[path = "fetch_add/register/native/byte.rs"]
mod cpu_xadd8;

#[path = "exchange/register/native/dword.rs"]
mod cpu_xchg32;

#[path = "exchange/register/native/byte.rs"]
mod cpu_xchg8;

#[path = "logical/locked/compiler/native/and32.rs"]
mod locked_and32_original;

#[path = "bit_test/locked/compiler/native/reset32.rs"]
mod locked_btr32_original;

#[path = "bit_test/locked/compiler/native/set32.rs"]
mod locked_bts32_original;

#[path = "compare_exchange/locked/compiler/native/word.rs"]
mod locked_cmpxchg16_original;

#[path = "compare_exchange/locked/compiler/native/byte.rs"]
mod locked_cmpxchg8_original;

#[path = "logical/flags/compiler/native/dynamic32.rs"]
mod locked_dynamic_flags32_original;

#[path = "logical/flags/compiler/native/full_mask32.rs"]
mod locked_fullmask32_original;

#[path = "logical/locked/compiler/native/or32.rs"]
mod locked_or32_original;

#[path = "fetch_add/locked/compiler/native/word.rs"]
mod locked_xadd16_original;

#[path = "fetch_add/locked/compiler/native/byte.rs"]
mod locked_xadd8_original;

#[path = "logical/locked/compiler/native/xor32.rs"]
mod locked_xor32_original;

#[path = "logical/flags/compiler/native/returned32.rs"]
mod returned_atomic_flags32_original;
