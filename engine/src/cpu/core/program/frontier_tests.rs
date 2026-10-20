use super::*;
use crate::memory::{PageRange, Permissions};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x2000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
}

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

#[test]
fn a_linear_frontier_excludes_the_existing_successor_body() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x83, 0x03, 2, 0x0f, 0x0b])
        .unwrap();
    let code = compile_with_frontier(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[],
        &[
            GuestAddress(0x2000),
            GuestAddress(0x1001),
            GuestAddress(0x1000),
            GuestAddress(0x1001),
            GuestAddress(0),
        ],
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 1);
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
    assert!(code.native_instructions().is_empty());
    assert!(code.is_current(&memory));
}

#[test]
fn both_conditional_paths_stop_at_the_same_retained_tail() {
    let mut memory = memory();
    memory
        .write(
            GuestAddress(0x1100),
            &[0x85, 0xc0, 0x74, 0x0c, 0x83, 0x03, 1, 0xeb, 0x17],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1110), &[0x83, 0x03, 2, 0xeb, 0x0b])
        .unwrap();
    memory
        .write(GuestAddress(0x1120), &[0x83, 0x03, 4, 0x0f, 0x0b])
        .unwrap();
    let code = compile_with_frontier(
        &memory,
        GuestAddress(0x1100),
        options(),
        &[],
        &[GuestAddress(0x1120)],
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 6);
    assert_eq!(
        code.entrypoints(),
        &[
            GuestAddress(0x1100),
            GuestAddress(0x1104),
            GuestAddress(0x1110)
        ]
    );
}

#[test]
fn the_requested_entry_is_always_decoded_even_if_it_is_a_frontier() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1120), &[0x83, 0x03, 4, 0x0f, 0x0b])
        .unwrap();
    let code = compile_with_frontier(
        &memory,
        GuestAddress(0x1120),
        options(),
        &[],
        &[GuestAddress(0x1120)],
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 2);
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1120)]);
    assert!(matches!(
        compile_with_frontier(
            &memory,
            GuestAddress(0x1120),
            options(),
            &[(GuestAddress(0x1120), 0)],
            &[GuestAddress(0x1120)],
        ),
        Err(CompileError::InvalidLimits)
    ));
}

#[test]
fn a_frontier_target_has_its_own_source_authority() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xe9, 0xfb, 0x0f, 0, 0])
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0x0f, 0x0b]).unwrap();
    let prefix = compile_with_frontier(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[],
        &[GuestAddress(0x2000)],
    )
    .unwrap();
    let target = compile(&memory, GuestAddress(0x2000), options()).unwrap();
    assert_eq!(prefix.instruction_count(), 1);
    assert_eq!(prefix.entrypoints(), &[GuestAddress(0x1000)]);
    memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
    assert!(!target.is_current(&memory));
    assert!(prefix.is_current(&memory));
    memory.write(GuestAddress(0x1001), &[0xfa]).unwrap();
    assert!(!prefix.is_current(&memory));
}

#[test]
fn generated_frontier_modules_preserve_exact_prefixes_and_faults() {
    use std::{
        fmt::Write,
        fs,
        path::Path,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    let directory = std::env::temp_dir().join(format!(
        "ring3-core-frontier-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let mut memory = memory();
    let mut image = [0; 4096];
    image[..6].copy_from_slice(&[0x40, 0x83, 0x03, 2, 0x0f, 0x0b]);
    image[0x100..0x109].copy_from_slice(&[0x85, 0xc0, 0x74, 0x0c, 0x83, 0x03, 1, 0xeb, 0x17]);
    image[0x110..0x115].copy_from_slice(&[0x83, 0x03, 2, 0xeb, 0x0b]);
    image[0x120..0x125].copy_from_slice(&[0x83, 0x03, 4, 0x0f, 0x0b]);
    memory.write(GuestAddress(0x1000), &image).unwrap();
    fs::write(directory.join("code.bin"), image).unwrap();
    let mut manifest = String::new();
    for (name, entry, frontier) in [
        ("linear", 0x1000, vec![GuestAddress(0x1001)]),
        ("linear-tail", 0x1001, vec![GuestAddress(0x1001)]),
        ("fork", 0x1100, vec![GuestAddress(0x1120)]),
        ("fork-tail", 0x1120, vec![GuestAddress(0x1120)]),
    ] {
        let code =
            compile_with_frontier(&memory, GuestAddress(entry), options(), &[], &frontier).unwrap();
        fs::write(directory.join(format!("{name}.wasm")), code.wasm()).unwrap();
        writeln!(&mut manifest, "{name}\t{entry}\t{}", code.identity()).unwrap();
    }
    fs::write(directory.join("regions.tsv"), manifest).unwrap();
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cpu/core/fixtures/frontier.mjs"))
        .arg(&directory)
        .output()
        .expect("run generated frontier fixture");
    assert!(
        result.status.success(),
        "frontier fixture retained at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    fs::remove_dir_all(directory).unwrap();
}
