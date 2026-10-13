use std::{
    fmt::Write as _,
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "native.rs"]
mod native;

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").unwrap();
    }
    output
}

#[test]
fn actual_canonical_guest_corpus_matches_native_publication() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .expect("set RING3_ENGINE_WASM to the coherent enabled AVX verification engine");
    assert!(engine.is_file());
    let output = root.join("target/p2-avx-corpus").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let cases = native::corpus();
    let mut rows = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        rows.push(format!("{{\"index\":{index},\"name\":\"{}\",\"original\":\"{}\",\"code\":\"{}\",\"descriptor\":{},\"operands\":{},\"immediate\":{},\"helper\":{},\"length\":{},\"access\":{},\"alignment\":{},\"selected\":{}}}", case.name, hex(&case.original), hex(&case.code), case.descriptor, case.operands, case.immediate, case.helper, case.length, case.access, case.alignment, case.selected));
    }
    fs::write(output.join("cases.json"), format!("[{}]\n", rows.join(","))).unwrap();
    for (name, frames) in [
        (
            "input.bin",
            cases
                .iter()
                .flat_map(|case| case.input.iter().copied())
                .collect::<Vec<_>>(),
        ),
        (
            "expected.bin",
            cases
                .iter()
                .flat_map(|case| case.expected.iter().copied())
                .collect(),
        ),
        (
            "ram.bin",
            cases
                .iter()
                .flat_map(|case| case.ram.iter().copied())
                .collect(),
        ),
        (
            "expected-ram.bin",
            cases
                .iter()
                .flat_map(|case| case.expected_ram.iter().copied())
                .collect(),
        ),
    ] {
        fs::write(output.join(name), frames).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/src/cpu/x86/avx/integration/fixtures/p2-avx-corpus/runtime/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .expect("node available for canonical guest integration proof");
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "canonical guest integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(
        stdout.contains("\"runs\":2752"),
        "nonempty complete688×4 guest proof: {stdout}"
    );
    println!("{}", stdout.trim());
}
