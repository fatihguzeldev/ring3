#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "native/instance.rs"]
mod engine_instance;

#[path = "native/exit.rs"]
mod process_exit;

#[path = "native/identity.rs"]
mod process_reusable_identity;
