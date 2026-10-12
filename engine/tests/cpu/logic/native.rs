#[path = "../../support/cpu/prefix/byte.rs"]
mod support_byte_prefix_admission;

#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/prefix/word.rs"]
mod support_word_prefix;

#[path = "compare/register/native/byte.rs"]
mod cpu_byte_compare;

#[path = "binary/register/native/byte.rs"]
mod cpu_byte_logical;

#[path = "test/register/native/byte.rs"]
mod cpu_byte_test;

#[path = "unary/register/native/byte.rs"]
mod cpu_byte_unary;

#[path = "binary/modify/native/byte.rs"]
mod cpu_memory_byte_logical;

#[path = "unary/memory/native/byte.rs"]
mod cpu_memory_byte_unary;

#[path = "compare/read/native/byte.rs"]
mod cpu_register_memory_byte_compare;

#[path = "binary/read/native/byte.rs"]
mod cpu_register_memory_byte_logical;

#[path = "binary/register/and/word/native.rs"]
mod cpu_word_and;

#[path = "compare/register/native/word.rs"]
mod cpu_word_compare;

#[path = "unary/register/inc_dec/word/native.rs"]
mod cpu_word_inc_dec;

#[path = "binary/modify/immediate/word/native.rs"]
mod cpu_word_memory_immediate_logical_store;

#[path = "binary/read/native/word.rs"]
mod cpu_word_memory_logical;

#[path = "binary/inputs/native/read.rs"]
mod cpu_word_memory_logical_legacy;

#[path = "binary/modify/register/word/native.rs"]
mod cpu_word_memory_logical_store;

#[path = "binary/inputs/native/store.rs"]
mod cpu_word_memory_logical_store_legacy;

#[path = "unary/memory/native/word.rs"]
mod cpu_word_memory_unary;

#[path = "unary/register/neg_not/word/native.rs"]
mod cpu_word_neg_not;

#[path = "binary/register/or_xor/word/native.rs"]
mod cpu_word_or_xor;

#[path = "test/register/native/word.rs"]
mod cpu_word_test;
