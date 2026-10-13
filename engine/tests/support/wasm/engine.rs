use std::path::{Path, PathBuf};

/// Select the canonical engine artifact while retaining the legacy local-build fallback.
pub fn selected_engine(root: &Path) -> PathBuf {
    std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"))
}
