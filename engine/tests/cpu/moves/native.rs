#[path = "../../support/cpu/admission/native.rs"]
mod support_admission;

#[path = "../../support/cpu/wasm/native.rs"]
mod support_wasm_interface;

#[path = "../../support/cpu/fetch/native.rs"]
mod support_fetch;

#[path = "../../support/cpu/prefix/byte.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "memory/load/native/byte.rs"]
mod cpu_byte_load;

#[path = "addressing/moffs/native/byte.rs"]
mod cpu_byte_moffs;

#[path = "register/native/byte.rs"]
mod cpu_byte_move;

#[path = "register/immediate/byte/native.rs"]
mod cpu_byte_move_immediate_modrm;

#[path = "memory/store/native/byte.rs"]
mod cpu_byte_store;

#[path = "extend/accumulator/native/cdq.rs"]
mod cpu_cdq;

#[path = "extend/accumulator/native/cwde.rs"]
mod cpu_cwde;

#[path = "extend/accumulator/native/word.rs"]
mod cpu_word_accumulator_extend;

#[path = "extend/source/byte/word/native.rs"]
mod cpu_word_byte_extend;

#[path = "addressing/lea/native/word.rs"]
mod cpu_word_lea;

#[path = "memory/word/native/operations.rs"]
mod cpu_word_memory_move;

#[path = "memory/word/native/admission.rs"]
mod cpu_word_memory_move_bound;

#[path = "addressing/moffs/native/word.rs"]
mod cpu_word_moffs;

#[path = "register/immediate/word/native.rs"]
mod cpu_word_move_immediate;

#[path = "exchange/register/native/word.rs"]
mod cpu_word_register_exchange;

#[path = "register/native/word.rs"]
mod cpu_word_register_move;

#[path = "endian/native/movbe.rs"]
mod cpu_movbe;
#[path = "addressing/lookup/native/xlat.rs"]
mod cpu_xlat;
