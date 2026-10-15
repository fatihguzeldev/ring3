mod catalog;
mod interfaces;
mod module;
mod result;
mod presentation;
mod adapter;

pub use catalog::{CREATE_GATE, Method, MethodGate};
pub use interfaces::InterfaceKind;
pub use module::system_module;
pub use result::HResult;
pub use presentation::{PresentationParameters32, PRESENT_PARAMETER_WORDS, SDK_VERSION, validate_behavior};
pub use adapter::{AdapterProfile32, DisplayMode32, CAPS_WORDS, ADAPTER_IDENTIFIER_BYTES, is_display_format, is_render_format};
