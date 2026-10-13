#[path = "dispatch/support/switch.rs"]
#[allow(dead_code)]
mod support_compiled_switch;

#[path = "../support/loader/pe32/image.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "records/support/compiled.rs"]
#[allow(dead_code)]
mod support_pe32_compiled_records;

#[path = "records/support/mutable.rs"]
#[allow(dead_code)]
mod support_pe32_mutable_records;

#[path = "sort/support/pe32.rs"]
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

#[path = "atomic/flags/native.rs"]
mod returned_atomic_flags32_original;
