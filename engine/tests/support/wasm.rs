#[path = "wasm/engine.rs"]
mod engine;

pub use engine::selected_engine;

use std::{
    path::Path,
    process::{Command, Output},
};

/// run a fixture against the selected engine without changing its inputs or oracle.
pub fn run_node(
    root: &Path,
    runner: &str,
    engine: &Path,
    output: &Path,
    expected_sha256: Option<&str>,
) -> Output {
    let mut command = Command::new("node");
    command
        .arg(root.join(runner))
        .arg(engine)
        .arg(output)
        .arg(root);
    if let Some(expected) = expected_sha256 {
        command.env("RING3_ENGINE_SHA256", expected);
    }
    command.output().expect("run fixture with node")
}
