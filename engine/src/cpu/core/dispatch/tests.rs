use super::*;
use crate::{
    cpu::core::{CompileOptions, compile, compile_with_frontier, compile_with_gates},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    fmt::Write,
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn generated_dispatch_chains_unchanged_regions_with_one_shared_work_budget() {
    let directory = std::env::temp_dir().join(format!(
        "ring3-core-dispatch-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("dispatch.wasm"), emit_dispatch()).unwrap();
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut image = [0; 4096];
    image[..10].copy_from_slice(&[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b]);
    image[0x40..0x4a].copy_from_slice(&[0x83, 0xc0, 3, 0x31, 0xc2, 0x83, 0x43, 0x0c, 1, 0xc3]);
    image[0x100..0x104].copy_from_slice(&[0xf3, 0xa4, 0xff, 0xe2]);
    image[0x140..0x145].copy_from_slice(&[0x83, 0xc0, 1, 0x0f, 0x0b]);
    image[0x200..0x204].copy_from_slice(&[0x89, 0x03, 0xff, 0xe2]);
    image[0x300..0x304].copy_from_slice(&[0x8b, 0x03, 0xff, 0xe2]);
    image[0x400..0x402].copy_from_slice(&[0x0f, 0x0b]);
    memory.write(GuestAddress(0x1000), &image).unwrap();
    fs::write(directory.join("code.bin"), image).unwrap();
    let mut manifest = String::new();
    for (name, entry) in [
        ("caller", 0x1000),
        ("callee", 0x1040),
        ("repeat", 0x1100),
        ("after-repeat", 0x1140),
        ("code-store", 0x1200),
        ("read-fault", 0x1300),
        ("gate", 0x1400),
    ] {
        let options = CompileOptions {
            instructions: 64,
            blocks: 8,
            ..CompileOptions::default()
        };
        let code = if name == "gate" {
            compile_with_gates(
                &memory,
                GuestAddress(entry),
                options,
                &[(GuestAddress(entry), 77)],
            )
            .unwrap()
        } else if name == "caller" {
            compile_with_frontier(
                &memory,
                GuestAddress(entry),
                options,
                &[],
                &[GuestAddress(0x1040)],
            )
            .unwrap()
        } else {
            compile(&memory, GuestAddress(entry), options).unwrap()
        };
        fs::write(directory.join(format!("{name}.wasm")), code.wasm()).unwrap();
        writeln!(&mut manifest, "{name}\t{entry}\t{}", code.identity()).unwrap();
    }
    fs::write(directory.join("regions.tsv"), manifest).unwrap();
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cpu/core/fixtures/dispatch.mjs"))
        .arg(&directory)
        .output()
        .expect("run generated dispatch fixture");
    assert!(
        result.status.success(),
        "dispatch fixture retained at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    fs::remove_dir_all(directory).unwrap();
}
