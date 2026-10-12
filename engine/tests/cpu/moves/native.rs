#[path = "../../support/cpu/byte_prefix_admission.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
mod support_word_prefix;

#[path = "cases/native/cpu_byte_load.rs"]
mod cpu_byte_load;

#[path = "cases/native/cpu_byte_moffs.rs"]
mod cpu_byte_moffs;

#[path = "cases/native/cpu_byte_move.rs"]
mod cpu_byte_move;

#[path = "cases/native/cpu_byte_move_immediate_modrm.rs"]
mod cpu_byte_move_immediate_modrm;

#[path = "cases/native/cpu_byte_store.rs"]
mod cpu_byte_store;

#[path = "cases/native/cpu_cdq.rs"]
mod cpu_cdq;

#[path = "cases/native/cpu_cwde.rs"]
mod cpu_cwde;

#[path = "cases/native/cpu_word_accumulator_extend.rs"]
mod cpu_word_accumulator_extend;

#[path = "cases/native/cpu_word_byte_extend.rs"]
mod cpu_word_byte_extend;

#[path = "cases/native/cpu_word_lea.rs"]
mod cpu_word_lea;

#[path = "cases/native/cpu_word_memory_move.rs"]
mod cpu_word_memory_move;

#[path = "cases/native/cpu_word_memory_move_bound.rs"]
mod cpu_word_memory_move_bound;

#[path = "cases/native/cpu_word_moffs.rs"]
mod cpu_word_moffs;

#[path = "cases/native/cpu_word_move_immediate.rs"]
mod cpu_word_move_immediate;

#[path = "cases/native/cpu_word_register_exchange.rs"]
mod cpu_word_register_exchange;

#[path = "cases/native/cpu_word_register_move.rs"]
mod cpu_word_register_move;

#[path = "cases/native/cpu_xlat.rs"]
mod cpu_xlat;
