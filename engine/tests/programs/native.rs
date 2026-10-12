#[path = "../support/loader/compiled_switch.rs"]
#[allow(dead_code)]
mod support_compiled_switch;

#[path = "../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../support/loader/pe32_compiled_records.rs"]
#[allow(dead_code)]
mod support_pe32_compiled_records;

#[path = "../support/loader/pe32_mutable_records.rs"]
#[allow(dead_code)]
mod support_pe32_mutable_records;

#[path = "../support/loader/pe32_sort.rs"]
#[allow(dead_code)]
mod support_pe32_sort;

#[path = "cases/native/bitmap_dispatch32_original.rs"]
mod bitmap_dispatch32_original;

#[path = "cases/native/compiled_numeric_original.rs"]
mod compiled_numeric_original;

#[path = "cases/native/compiled_switch.rs"]
mod compiled_switch;

#[path = "cases/native/pe32_compiled_records.rs"]
mod pe32_compiled_records;

#[path = "cases/native/pe32_mutable_records.rs"]
mod pe32_mutable_records;

#[path = "cases/native/pe32_sort.rs"]
mod pe32_sort;
