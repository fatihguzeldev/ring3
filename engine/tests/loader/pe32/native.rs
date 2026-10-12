#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/loader/pe32_relocation.rs"]
#[allow(dead_code)]
mod support_pe32_relocation;

#[path = "cases/native/pe32_declarations_admission.rs"]
mod pe32_declarations_admission;

#[path = "cases/native/pe32_dll_characteristics.rs"]
mod pe32_dll_characteristics;

#[path = "cases/native/pe32_loader.rs"]
mod pe32_loader;

#[path = "cases/native/pe32_relocation.rs"]
mod pe32_relocation;
