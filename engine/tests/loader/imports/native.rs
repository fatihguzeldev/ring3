#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "native/v1.rs"]
mod pe32_imports;

#[path = "native/v2.rs"]
mod pe32_imports_v2;

#[path = "native/v3.rs"]
mod pe32_imports_v3;

#[path = "native/v4.rs"]
mod pe32_imports_v4;

#[path = "native/v5.rs"]
mod pe32_imports_v5;

#[path = "native/v6.rs"]
mod pe32_linked_v6;

#[path = "native/v7.rs"]
mod pe32_linked_v7;

#[path = "native/v8.rs"]
mod pe32_linked_v8;
