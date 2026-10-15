use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

static ARTIFACT: OnceLock<(PathBuf, String)> = OnceLock::new();

fn artifact() -> &'static (PathBuf, String) {
    ARTIFACT.get_or_init(|| {
        let path = std::env::var_os("RING3_ENGINE_WASM")
            .map(PathBuf::from)
            .expect("run python3 engine/tests/support/wasm/run.py <wasm-test-target>");
        let path = path.canonicalize().expect("selected engine artifact must exist");
        let expected = std::env::var("RING3_ENGINE_SHA256")
            .expect("select the engine artifact and its exact SHA-256 together");
        assert!(expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "selected engine SHA-256 must contain 64 hexadecimal digits");
        let output = Command::new("node").arg("-e").arg(
            "const fs=require('node:fs'),c=require('node:crypto');process.stdout.write(c.createHash('sha256').update(fs.readFileSync(process.argv[1])).digest('hex'))"
        ).arg(&path).output().expect("validate selected engine SHA-256 with node");
        assert!(output.status.success(), "could not hash selected engine: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected.to_ascii_lowercase(),
            "selected engine artifact does not match its SHA-256");
        (path, expected)
    })
}

pub fn selected_engine(_root: &Path) -> PathBuf {
    artifact().0.clone()
}
