use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32_sort.rs"]
#[allow(dead_code)]
mod pe;

#[test]
fn actual_resident_pe_sorts_and_checksums_without_host_cpu_reseed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let folder = root.join("target/p2-pe32-sort-fixtures").join(unique);
    let fixture = pe::fixture();
    for campaign in ["first", "second"] {
        let output = folder.join(campaign);
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("sort.exe"), &fixture.image).unwrap();
        fs::write(output.join("manifest.json"), fixture.manifest_json()).unwrap();
        let result = Command::new("node")
            .arg(root.join("engine/tests/fixtures/p2-pe32-sort/run.mjs"))
            .arg(&engine)
            .arg(&output)
            .arg(root)
            .arg(campaign)
            .output()
            .unwrap();
        fs::write(output.join("node-stdout-first.txt"), &result.stdout).unwrap();
        fs::write(output.join("node-stderr-first.txt"), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "actual PE sort/checksum campaign failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    }
}
