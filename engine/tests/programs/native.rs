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

#[path = "dispatch/native/bitmap.rs"]
mod bitmap_dispatch32_original;

#[path = "numeric/native/compiled.rs"]
mod compiled_numeric_original;

#[path = "dispatch/native/switch.rs"]
mod compiled_switch;

#[path = "records/native/compiled.rs"]
mod pe32_compiled_records;

#[path = "records/native/mutable.rs"]
mod pe32_mutable_records;

#[path = "sort/native.rs"]
mod pe32_sort;
