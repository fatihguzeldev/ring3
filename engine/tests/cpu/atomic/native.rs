#[path = "../../support/cpu/byte_prefix_admission.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
mod support_word_prefix;

#[path = "cases/native/cpu_cmpxchg32.rs"]
mod cpu_cmpxchg32;

#[path = "cases/native/cpu_cmpxchg8.rs"]
mod cpu_cmpxchg8;

#[path = "cases/native/cpu_lock_exchange_prefixes.rs"]
mod cpu_lock_exchange_prefixes;

#[path = "cases/native/cpu_locked_and32.rs"]
mod cpu_locked_and32;

#[path = "cases/native/cpu_locked_btr32.rs"]
mod cpu_locked_btr32;

#[path = "cases/native/cpu_locked_bts32.rs"]
mod cpu_locked_bts32;

#[path = "cases/native/cpu_locked_cmpxchg16.rs"]
mod cpu_locked_cmpxchg16;

#[path = "cases/native/cpu_locked_cmpxchg32.rs"]
mod cpu_locked_cmpxchg32;

#[path = "cases/native/cpu_locked_cmpxchg8.rs"]
mod cpu_locked_cmpxchg8;

#[path = "cases/native/cpu_locked_logical_imm32.rs"]
mod cpu_locked_logical_imm32;

#[path = "cases/native/cpu_locked_logical_register32.rs"]
mod cpu_locked_logical_register32;

#[path = "cases/native/cpu_locked_or32.rs"]
mod cpu_locked_or32;

#[path = "cases/native/cpu_locked_xadd16.rs"]
mod cpu_locked_xadd16;

#[path = "cases/native/cpu_locked_xadd32.rs"]
mod cpu_locked_xadd32;

#[path = "cases/native/cpu_locked_xadd8.rs"]
mod cpu_locked_xadd8;

#[path = "cases/native/cpu_locked_xor32.rs"]
mod cpu_locked_xor32;

#[path = "cases/native/cpu_memory_bit_mutation32.rs"]
mod cpu_memory_bit_mutation32;

#[path = "cases/native/cpu_memory_bit_mutation_bridge.rs"]
mod cpu_memory_bit_mutation_bridge;

#[path = "cases/native/cpu_memory_cmpxchg32.rs"]
mod cpu_memory_cmpxchg32;

#[path = "cases/native/cpu_memory_cmpxchg8.rs"]
mod cpu_memory_cmpxchg8;

#[path = "cases/native/cpu_memory_xadd32.rs"]
mod cpu_memory_xadd32;

#[path = "cases/native/cpu_memory_xadd8.rs"]
mod cpu_memory_xadd8;

#[path = "cases/native/cpu_memory_xchg16.rs"]
mod cpu_memory_xchg16;

#[path = "cases/native/cpu_memory_xchg32.rs"]
mod cpu_memory_xchg32;

#[path = "cases/native/cpu_memory_xchg8.rs"]
mod cpu_memory_xchg8;

#[path = "cases/native/cpu_word_cmpxchg.rs"]
mod cpu_word_cmpxchg;

#[path = "cases/native/cpu_word_memory_cmpxchg.rs"]
mod cpu_word_memory_cmpxchg;

#[path = "cases/native/cpu_word_memory_xadd.rs"]
mod cpu_word_memory_xadd;

#[path = "cases/native/cpu_word_xadd.rs"]
mod cpu_word_xadd;

#[path = "cases/native/cpu_xadd32.rs"]
mod cpu_xadd32;

#[path = "cases/native/cpu_xadd8.rs"]
mod cpu_xadd8;

#[path = "cases/native/cpu_xchg32.rs"]
mod cpu_xchg32;

#[path = "cases/native/cpu_xchg8.rs"]
mod cpu_xchg8;

#[path = "cases/native/locked_and32_original.rs"]
mod locked_and32_original;

#[path = "cases/native/locked_btr32_original.rs"]
mod locked_btr32_original;

#[path = "cases/native/locked_bts32_original.rs"]
mod locked_bts32_original;

#[path = "cases/native/locked_cmpxchg16_original.rs"]
mod locked_cmpxchg16_original;

#[path = "cases/native/locked_cmpxchg8_original.rs"]
mod locked_cmpxchg8_original;

#[path = "cases/native/locked_dynamic_flags32_original.rs"]
mod locked_dynamic_flags32_original;

#[path = "cases/native/locked_fullmask32_original.rs"]
mod locked_fullmask32_original;

#[path = "cases/native/locked_or32_original.rs"]
mod locked_or32_original;

#[path = "cases/native/locked_xadd16_original.rs"]
mod locked_xadd16_original;

#[path = "cases/native/locked_xadd8_original.rs"]
mod locked_xadd8_original;

#[path = "cases/native/locked_xor32_original.rs"]
mod locked_xor32_original;

#[path = "cases/native/returned_atomic_flags32_original.rs"]
mod returned_atomic_flags32_original;
