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

#[path = "../support/wasm.rs"]
mod support_wasm;

#[path = "dispatch/wasm/switch.rs"]
mod compiled_switch_wasm;

#[path = "dispatch/wasm/bitmap.rs"]
mod cpu_bitmap_dispatch32_wasm;

#[path = "atomic/flags/wasm.rs"]
mod cpu_returned_atomic_flags32_wasm;

#[path = "numeric/wasm/compiled.rs"]
mod fp_compiled_wasm;

#[path = "records/wasm/compiled.rs"]
mod pe32_compiled_records_wasm;

#[path = "records/wasm/mutable.rs"]
mod pe32_mutable_records_wasm;

#[path = "sort/wasm.rs"]
mod pe32_sort_wasm;
