#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "startup/wasm/c_imports.rs"]
mod pe32_c_imports_wasm;

#[path = "startup/wasm/fp_allocation.rs"]
mod pe32_fp_alloc_wasm;

#[path = "startup/wasm/fp_imports.rs"]
mod pe32_fp_imports_wasm;

#[path = "startup/wasm/sse2.rs"]
mod pe32_sse2_startup_wasm;

#[path = "input/wasm/copied.rs"]
mod process_image_input_wasm;

#[path = "startup/wasm/entry.rs"]
mod process_image_startup_wasm;

#[path = "wasm/imports.rs"]
mod process_pe32_imports_wasm;

#[path = "wasm/relocatable.rs"]
mod process_pe32_relocatable_wasm;

#[path = "wasm/relocation.rs"]
mod process_pe32_relocation_wasm;

#[path = "wasm/terminal.rs"]
mod process_pe32_terminal_wasm;

#[path = "wasm/tsaware.rs"]
mod process_pe32_tsaware_wasm;

#[path = "wasm/load.rs"]
mod process_pe32_wasm;
