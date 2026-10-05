use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_wasm_restores_checked_leave_frames_before_guest_return() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
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
    let output = root.join("target/p2-leave-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("leave.x86"), [0xc9, 0xeb, 0, 0x0f, 0x0b]).unwrap();
    fs::write(
        output.join("retry.x86"),
        [0x47, 0xc9, 0x89, 0xef, 0xeb, 0, 0x0f, 0x0b],
    )
    .unwrap();
    let mut frame = vec![
        0xb8, 42, 0, 0, 0, 0xe8, 0x16, 0, 0, 0, 0xa3, 0, 0x30, 0, 0, 0xeb, 0, 0x0f, 0x0b,
    ];
    frame.resize(32, 0xcc);
    frame.extend_from_slice(&[
        0x55, 0x89, 0xe5, 0x8d, 0x64, 0x24, 0xf0, 0xc7, 0x45, 0xfc, 0x88, 0x77, 0x66, 0x55, 0xb8,
        0x78, 0x56, 0x34, 0x12, 0xc9, 0xc3,
    ]);
    assert_eq!(frame.len(), 53);
    fs::write(output.join("frame.x86"), frame).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-leave/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual LEAVE integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
