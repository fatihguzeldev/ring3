#[cfg(any(target_arch = "wasm32", test))]
pub(in crate::process) mod detection;
