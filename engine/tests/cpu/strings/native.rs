#[path = "../../support/cpu/string_prefix.rs"]
mod support_string_prefix;

#[path = "load/native/dword.rs"]
mod cpu_dword_load_string;

#[path = "move/native/dword.rs"]
mod cpu_dword_move_string;

#[path = "store/native/dword.rs"]
mod cpu_dword_store_string;

#[path = "compare/native/dword.rs"]
mod cpu_dword_string_compare;

#[path = "repeat/native/admission.rs"]
mod cpu_rep_strings;

#[path = "compare/native/byte.rs"]
mod cpu_string_compare;

#[path = "transfer/native/byte.rs"]
mod cpu_string_load_store;

#[path = "integration/native/byte.rs"]
mod cpu_strings;

#[path = "load/native/word.rs"]
mod cpu_word_load_string;

#[path = "move/native/word.rs"]
mod cpu_word_move_string;

#[path = "store/native/word.rs"]
mod cpu_word_store_string;

#[path = "compare/native/word.rs"]
mod cpu_word_string_compare;
