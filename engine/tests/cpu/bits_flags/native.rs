#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/word_prefix.rs"]
mod support_word_prefix;

#[path = "scan/register/native/dword.rs"]
mod cpu_bit_scan;

#[path = "test/register/native/dword.rs"]
mod cpu_bit_test32;

#[path = "byte_order/native/dword.rs"]
mod cpu_bswap32;

#[path = "flags/native/ah.rs"]
mod cpu_flags_ah;

#[path = "scan/memory/native/dword.rs"]
mod cpu_memory_bit_scan;

#[path = "test/memory/native/dword.rs"]
mod cpu_memory_bit_test32;

#[path = "count/native/trailing32.rs"]
mod cpu_tzcnt32;

#[path = "scan/register/native/word.rs"]
mod cpu_word_bit_scan;

#[path = "test/register/native/word.rs"]
mod cpu_word_bit_test;

#[path = "test/memory/native/word.rs"]
mod cpu_word_memory_bit_test;
#[path = "count/native/forms.rs"]
mod cpu_bit_counts;

#[path = "manipulation/native/bmi1.rs"]
mod cpu_bmi1;

#[path = "manipulation/native/bmi2.rs"]
mod cpu_bmi2;

#[path = "checksum/native/crc32.rs"]
mod cpu_crc32;
