use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

fn shift_forms() -> Vec<Vec<u8>> {
    let mut forms = Vec::new();
    for extension in [4, 5, 7] {
        for opcode in [0xd0, 0xc0, 0xd2] {
            for destination in 0..8 {
                let mut bytes = vec![opcode, 0xc0 | extension << 3 | destination];
                if opcode == 0xc0 {
                    bytes.push(0xff);
                }
                forms.push(bytes);
            }
        }
    }
    assert_eq!(forms.len(), 72);
    forms
}

fn standalone_batches(output: &Path) {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut batches: Vec<_> = shift_forms()
        .chunks(63)
        .enumerate()
        .map(|(index, forms)| {
            let mut bytes: Vec<u8> = forms.iter().flatten().copied().collect();
            bytes.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
            let entry = GuestAddress(0x1000 + index as u32 * 0x200);
            assert!(entry.0 + bytes.len() as u32 <= 0x2000);
            memory.write(entry, &bytes).unwrap();
            (entry, bytes)
        })
        .collect();
    assert_eq!(batches.len(), 2);
    let mut boundary = Vec::new();
    for extension in [4, 5, 7] {
        for count in [0, 1, 2, 7, 8, 9, 31, 32, 33, 255] {
            boundary.extend_from_slice(&[0xc0, 0xc0 | extension << 3, count]);
        }
    }
    boundary.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    memory.write(GuestAddress(0x1400), &boundary).unwrap();
    batches.push((GuestAddress(0x1400), boundary));
    let chain = [
        0xb0, 0xff, 0xb1, 0x01, 0x00, 0xc8, 0xb1, 0x20, 0xd2, 0xe4, 0x10, 0xe8, 0xd0, 0xe8, 0x10,
        0xed, 0xd0, 0xfc, 0x0f, 0x92, 0xc2, 0xeb, 0x00, 0x0f, 0x0b,
    ];
    memory.write(GuestAddress(0x2000), &chain).unwrap();
    // all code is present before snapshots are bound to this address space.
    for (index, (entry, bytes)) in batches.iter().enumerate() {
        let compiled = compile_region(
            &memory,
            &[BlockSpec {
                entry: *entry,
                byte_length: bytes.len() as u32 - 2,
            }],
            CompileLimits::default(),
        )
        .unwrap();
        fs::write(output.join(format!("batch-{index}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("batch-{index}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let compiled = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x2000),
            byte_length: 23,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    fs::write(output.join("chain.x86"), chain).unwrap();
    fs::write(
        output.join("chain.wasm"),
        compiled.wasm_bytes(&memory).unwrap(),
    )
    .unwrap();
}

fn shift_image() -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0xa00, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 0x600);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    pe::put32(&mut bytes, pe::OPTIONAL + 104, 0x3100);
    pe::put32(&mut bytes, pe::OPTIONAL + 108, 40);
    pe::put32(&mut bytes, pe::OPTIONAL + 136, 0x5000);
    pe::put32(&mut bytes, pe::OPTIONAL + 140, 20);
    pe::put32(&mut bytes, pe::OPTIONAL + 192, 0x3150);
    pe::put32(&mut bytes, pe::OPTIONAL + 196, 8);
    let program: &[u8] = &[
        0xb8, 0x81, 0x40, 0x34, 0x12, 0xb9, 0x01, 0x80, 0x45, 0x23, 0xd0, 0xe4, 0x0f, 0x90, 0xc3,
        0x71, 0x41, 0xd0, 0xe8, 0xb1, 0x20, 0xd2, 0xe0, 0x0f, 0x90, 0xc3, 0xb1, 0x01, 0x0f, 0x92,
        0xc7, 0x73, 0x31, 0xd2, 0xfd, 0x0f, 0x98, 0xc6, 0x79, 0x2a, 0x0f, 0x93, 0xc2, 0x72, 0x25,
        0x3d, 0x40, 0x80, 0x34, 0x12, 0x75, 0x1e, 0xbe, 0x0b, 0x00, 0xc0, 0xc8, 0x89, 0x35, 0x00,
        0x30, 0x40, 0x00, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0xc7, 0x05, 0x04, 0x30, 0x40,
        0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b, 0xbe, 0xde, 0xc0, 0xad, 0xde, 0x89, 0x35, 0x00,
        0x30, 0x40, 0x00, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 102);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 1024);
    pe::put32(&mut bytes, pe::section(1) + 16, 1024);
    bytes[pe::DATA_RAW..0x800].fill(0);
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    pe::put32(&mut bytes, 0x540, 0x3170);
    pe::put32(&mut bytes, 0x550, 0x3170);
    bytes[0x560..0x56d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x572..0x57e].copy_from_slice(b"ExitProcess\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 20);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, 0x800);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x4000_0040);
    pe::put32(&mut bytes, 0x800, 0x1000);
    pe::put32(&mut bytes, 0x804, 20);
    for (index, offset) in [59, 66, 72, 89, 96, 0].into_iter().enumerate() {
        pe::put16(
            &mut bytes,
            0x808 + index * 2,
            if offset == 0 { 0 } else { 0x3000 | offset },
        );
    }
    bytes
}

#[test]
fn byte_shift_results_and_flags_execute_in_three_owners_and_copied_pe_paths() {
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
    let output = root.join("target/p2-byte-shift-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    standalone_batches(&output);
    fs::write(output.join("shift.exe"), shift_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-shift/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual byte shift integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
