#[path = "startup/native/c_imports.rs"]
mod pe32_c_imports;

#[path = "startup/native/fp_allocation.rs"]
mod pe32_fp_alloc;

#[path = "startup/native/fp_imports.rs"]
mod pe32_fp_imports;

#[path = "startup/native/sse2.rs"]
mod pe32_sse2_startup;

#[path = "input/native/copied.rs"]
mod process_image_input;

#[path = "startup/native/entry.rs"]
mod process_image_startup;

#[path = "input/native/original.rs"]
mod process_pe32_input;
