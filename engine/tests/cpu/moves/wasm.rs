#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "memory/load/wasm/byte.rs"]
mod cpu_byte_load_wasm;

#[path = "addressing/moffs/wasm/byte.rs"]
mod cpu_byte_moffs_wasm;

#[path = "register/wasm/byte_immediate.rs"]
mod cpu_byte_move_immediate_modrm_wasm;

#[path = "register/wasm/byte.rs"]
mod cpu_byte_move_wasm;

#[path = "memory/store/wasm/byte.rs"]
mod cpu_byte_store_wasm;

#[path = "extend/accumulator/wasm/cdq.rs"]
mod cpu_cdq_wasm;

#[path = "extend/accumulator/wasm/cwde.rs"]
mod cpu_cwde_wasm;

#[path = "extend/accumulator/wasm/word.rs"]
mod cpu_word_accumulator_extend_wasm;

#[path = "extend/source/wasm/byte_word.rs"]
mod cpu_word_byte_extend_wasm;

#[path = "addressing/lea/wasm/word.rs"]
mod cpu_word_lea_wasm;

#[path = "memory/word/wasm/operations.rs"]
mod cpu_word_memory_move_wasm;

#[path = "addressing/moffs/wasm/word.rs"]
mod cpu_word_moffs_wasm;

#[path = "register/wasm/word_immediate.rs"]
mod cpu_word_move_immediate_wasm;

#[path = "exchange/register/wasm/word.rs"]
mod cpu_word_register_exchange_wasm;

#[path = "register/wasm/word.rs"]
mod cpu_word_register_move_wasm;

#[path = "addressing/lookup/wasm/xlat.rs"]
mod cpu_xlat_wasm;
