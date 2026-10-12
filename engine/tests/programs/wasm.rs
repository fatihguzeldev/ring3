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

#[path = "cases/wasm/compiled_switch_wasm.rs"]
mod compiled_switch_wasm;

#[path = "cases/wasm/cpu_bitmap_dispatch32_wasm.rs"]
mod cpu_bitmap_dispatch32_wasm;

#[path = "cases/wasm/cpu_returned_atomic_flags32_wasm.rs"]
mod cpu_returned_atomic_flags32_wasm;

#[path = "cases/wasm/fp_compiled_wasm.rs"]
mod fp_compiled_wasm;

#[path = "cases/wasm/pe32_compiled_records_wasm.rs"]
mod pe32_compiled_records_wasm;

#[path = "cases/wasm/pe32_mutable_records_wasm.rs"]
mod pe32_mutable_records_wasm;

#[path = "cases/wasm/pe32_sort_wasm.rs"]
mod pe32_sort_wasm;
