#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/loader/pe32_relocation.rs"]
#[allow(dead_code)]
mod support_pe32_relocation;

#[path = "native/declarations.rs"]
mod pe32_declarations_admission;

#[path = "native/characteristics.rs"]
mod pe32_dll_characteristics;

#[path = "native/image.rs"]
mod pe32_loader;

#[path = "native/relocation.rs"]
mod pe32_relocation;
