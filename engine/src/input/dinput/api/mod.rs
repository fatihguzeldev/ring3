mod gates;
mod interfaces;
pub use gates::{FactoryGate, MethodGate};
pub use interfaces::{InterfaceKind32, SYSTEM_KEYBOARD, SYSTEM_MOUSE, object_selector};

mod modules;
pub use modules::system_module;
