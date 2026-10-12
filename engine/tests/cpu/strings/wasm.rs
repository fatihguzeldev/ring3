#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "load/wasm/dword.rs"]
mod cpu_dword_load_string_wasm;

#[path = "move/wasm/dword.rs"]
mod cpu_dword_move_string_wasm;

#[path = "store/wasm/dword.rs"]
mod cpu_dword_store_string_wasm;

#[path = "compare/wasm/dword.rs"]
mod cpu_dword_string_compare_wasm;

#[path = "compare/wasm/byte.rs"]
mod cpu_string_compare_wasm;

#[path = "transfer/wasm/byte.rs"]
mod cpu_string_load_store_wasm;

#[path = "integration/wasm/byte.rs"]
mod cpu_strings_wasm;

#[path = "load/wasm/word.rs"]
mod cpu_word_load_string_wasm;

#[path = "move/wasm/word.rs"]
mod cpu_word_move_string_wasm;

#[path = "store/wasm/word.rs"]
mod cpu_word_store_string_wasm;

#[path = "compare/wasm/word.rs"]
mod cpu_word_string_compare_wasm;
