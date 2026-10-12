use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        Register32,
        decode::decode_one,
        ir::{EffectiveAddress, Operation},
        simd::{PackedKind, ShuffleKind, SimdKind, SimdOperand, SimdOperation, SimdRegister},
    },
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

fn check_case(
    output: &Path,
    modules: &mut Vec<String>,
    bytes: &[u8],
    expected: SimdOperation,
    key: u64,
) {
    let length = bytes.len();
    let mut bytes = bytes.to_vec();
    bytes.extend([0xeb, 0xfe]);
    let mut engine = EngineInstance::new(1, key).unwrap();
    engine.map(0x2000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x2000, bytes.len() as u32).unwrap();
    engine.protect(0x2000, 1, 4).unwrap();
    let before = engine.arena().to_vec();
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x2000)).unwrap();
    assert_eq!(usize::from(decoded.length()), length);
    assert_eq!(decoded.next_pc(), GuestAddress(0x2000 + length as u32));
    let Operation::Simd(operation) = decoded.operation() else {
        panic!("expected a typed direct integer operation");
    };
    assert_eq!(*operation, expected);
    assert_eq!(engine.arena(), before);

    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&0x2000_u32.to_le_bytes());
    if expected.memory_bytes == 0 {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
            .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    }
    let before = engine.arena().to_vec();
    let module = if expected.memory_bytes != 0 {
        let id = engine.compile_resident_entries(1, 0).unwrap().get();
        engine.resident_bytes(id).unwrap()
    } else {
        engine.compile(1).unwrap();
        engine.artifact_bytes().unwrap()
    };
    let name = format!("integer-{}.wasm", modules.len());
    fs::write(output.join(&name), module).unwrap();
    modules.push(format!("[\"{name}\",{}]", expected.memory_bytes));
    assert_eq!(engine.arena(), before);
    let memory = engine.memory().unwrap();
    assert_eq!(memory.mapped_pages(), 1);
    for address in [0, 0x2000, 0x5000] {
        assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
    }
}

#[test]
fn direct_integer_forms_validate_without_semantic_helper_imports() {
    let output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/direct-integer-fixtures")
        .join(format!(
            "native-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let mut modules = Vec::new();
    let register = |vector_bytes, index| {
        if vector_bytes == 8 {
            SimdRegister::Mmx(index)
        } else {
            SimdRegister::Xmm(index)
        }
    };

    for (kind, opcode, lane_bytes) in [
        (PackedKind::MultiplyHighSigned, 0xe5, 2),
        (PackedKind::MultiplyHighUnsigned, 0xe4, 2),
        (PackedKind::MultiplyUnsignedDword, 0xf4, 4),
        (PackedKind::SumAbsoluteByteDifferences, 0xf6, 1),
    ] {
        for vector_bytes in [8, 16] {
            for form in 0..3 {
                let mut bytes = Vec::new();
                if vector_bytes == 16 {
                    bytes.push(0x66);
                }
                let (modrm, source, memory_bytes) = match form {
                    0 => (0xf7, SimdOperand::Register(register(vector_bytes, 7)), 0),
                    1 => (0xf6, SimdOperand::Register(register(vector_bytes, 6)), 0),
                    2 => (
                        0x30,
                        SimdOperand::Memory(EffectiveAddress {
                            base: Some(Register32::Eax),
                            ..Default::default()
                        }),
                        u16::from(vector_bytes),
                    ),
                    _ => unreachable!(),
                };
                bytes.extend([0x0f, opcode, modrm]);
                check_case(
                    &output,
                    &mut modules,
                    &bytes,
                    SimdOperation {
                        kind: SimdKind::Packed(kind),
                        destination: SimdOperand::Register(register(vector_bytes, 6)),
                        source,
                        vector_bytes,
                        lane_bytes,
                        memory_bytes,
                        immediate: None,
                        aligned: form == 2 && vector_bytes == 16,
                        scalar: false,
                        implicit_address: None,
                    },
                    0x4849_4d55_4c33_3201,
                );
            }
        }
    }
    assert_eq!(modules.len(), 24);

    for (kind, lane, variable_opcode, immediate_opcode, immediate_group) in [
        (PackedKind::ShiftLeft, 2, 0xf1, 0x71, 6),
        (PackedKind::ShiftLeft, 4, 0xf2, 0x72, 6),
        (PackedKind::ShiftLeft, 8, 0xf3, 0x73, 6),
        (PackedKind::ShiftRight, 2, 0xd1, 0x71, 2),
        (PackedKind::ShiftRight, 4, 0xd2, 0x72, 2),
        (PackedKind::ShiftRight, 8, 0xd3, 0x73, 2),
        (PackedKind::ShiftRightSigned, 2, 0xe1, 0x71, 4),
        (PackedKind::ShiftRightSigned, 4, 0xe2, 0x72, 4),
    ] {
        for vector_bytes in [8, 16] {
            for form in 0..3 {
                let mut bytes = Vec::new();
                if vector_bytes == 16 {
                    bytes.push(0x66);
                }
                let (source, immediate, memory_bytes) = match form {
                    0 => {
                        bytes.extend([0x0f, immediate_opcode, 0xc6 | immediate_group << 3, 0xff]);
                        (SimdOperand::None, Some(0xff), 0)
                    }
                    1 => {
                        bytes.extend([0x0f, variable_opcode, 0xf7]);
                        (SimdOperand::Register(register(vector_bytes, 7)), None, 0)
                    }
                    2 => {
                        bytes.extend([0x0f, variable_opcode, 0x30]);
                        (
                            SimdOperand::Memory(EffectiveAddress {
                                base: Some(Register32::Eax),
                                ..Default::default()
                            }),
                            None,
                            u16::from(vector_bytes),
                        )
                    }
                    _ => unreachable!(),
                };
                check_case(
                    &output,
                    &mut modules,
                    &bytes,
                    SimdOperation {
                        kind: SimdKind::Packed(kind),
                        destination: SimdOperand::Register(register(vector_bytes, 6)),
                        source,
                        vector_bytes,
                        lane_bytes: lane,
                        memory_bytes,
                        immediate,
                        aligned: form == 2 && vector_bytes == 16,
                        scalar: false,
                        implicit_address: None,
                    },
                    0x5348_4946_5433_3201,
                );
            }
        }
    }
    assert_eq!(modules.len(), 72);

    for (kind, lane, opcode) in [
        (ShuffleKind::PackSigned, 1, 0x63),
        (ShuffleKind::PackSigned, 2, 0x6b),
        (ShuffleKind::PackUnsigned, 1, 0x67),
    ] {
        for vector_bytes in [8, 16] {
            for form in 0..3 {
                let mut bytes = Vec::new();
                if vector_bytes == 16 {
                    bytes.push(0x66);
                }
                let (modrm, source, memory_bytes) = match form {
                    0 => (0xf7, SimdOperand::Register(register(vector_bytes, 7)), 0),
                    1 => (0xf6, SimdOperand::Register(register(vector_bytes, 6)), 0),
                    2 => (
                        0x30,
                        SimdOperand::Memory(EffectiveAddress {
                            base: Some(Register32::Eax),
                            ..Default::default()
                        }),
                        u16::from(vector_bytes),
                    ),
                    _ => unreachable!(),
                };
                bytes.extend([0x0f, opcode, modrm]);
                check_case(
                    &output,
                    &mut modules,
                    &bytes,
                    SimdOperation {
                        kind: SimdKind::Shuffle(kind),
                        destination: SimdOperand::Register(register(vector_bytes, 6)),
                        source,
                        vector_bytes,
                        lane_bytes: lane,
                        memory_bytes,
                        immediate: None,
                        aligned: form == 2 && vector_bytes == 16,
                        scalar: false,
                        implicit_address: None,
                    },
                    0x5041_434b_5333_3201,
                );
            }
        }
    }
    assert_eq!(modules.len(), 90);

    for (kind, group) in [
        (ShuffleKind::ShiftBytesLeft, 7),
        (ShuffleKind::ShiftBytesRight, 3),
    ] {
        for count in [0, 1, 15, 16, 255] {
            let bytes = [0x66, 0x0f, 0x73, 0xc6 | group << 3, count];
            check_case(
                &output,
                &mut modules,
                &bytes,
                SimdOperation {
                    kind: SimdKind::Shuffle(kind),
                    destination: SimdOperand::Register(SimdRegister::Xmm(6)),
                    source: SimdOperand::None,
                    vector_bytes: 16,
                    lane_bytes: 1,
                    memory_bytes: 0,
                    immediate: Some(count),
                    aligned: false,
                    scalar: false,
                    implicit_address: None,
                },
                0x5348_4946_5433_3201,
            );
        }
    }
    for vector_bytes in [8, 16] {
        for memory in [false, true] {
            let mut bytes = Vec::new();
            if vector_bytes == 16 {
                bytes.push(0x66);
            }
            bytes.extend([0x0f, 0xc4, if memory { 0x30 } else { 0xf0 }, 0xff]);
            check_case(
                &output,
                &mut modules,
                &bytes,
                SimdOperation {
                    kind: SimdKind::Shuffle(ShuffleKind::InsertWord),
                    destination: SimdOperand::Register(register(vector_bytes, 6)),
                    source: if memory {
                        SimdOperand::Memory(EffectiveAddress {
                            base: Some(Register32::Eax),
                            ..Default::default()
                        })
                    } else {
                        SimdOperand::Register(SimdRegister::Gpr(Register32::Eax))
                    },
                    vector_bytes,
                    lane_bytes: 2,
                    memory_bytes: if memory { 2 } else { 0 },
                    immediate: Some(0xff),
                    aligned: false,
                    scalar: false,
                    implicit_address: None,
                },
                0x5041_434b_5333_3201,
            );
        }
    }
    assert_eq!(modules.len(), 104);
    fs::write(
        output.join("modules.json"),
        format!("[{}]", modules.join(",")),
    )
    .unwrap();
    let validation = Command::new("node")
        .arg("-e")
        .arg(r#"
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const directory = process.argv[1], cases = JSON.parse(fs.readFileSync(path.join(directory, 'modules.json')));
const helperBacked = [];
for (const [name, memoryBytes] of cases) {
    const module = new WebAssembly.Module(fs.readFileSync(path.join(directory, name)));
    const imports = WebAssembly.Module.imports(module).filter(item => item.kind === 'function');
    assert.equal(imports.filter(item => item.name === 'cpu_read').length, memoryBytes ? 1 : 0, name);
    assert.equal(imports.filter(item => item.name.startsWith('cpu_write')).length, 0, name);
    if (imports.some(item => item.name.startsWith('simd_execute'))) helperBacked.push(name);
}
console.log(JSON.stringify({validated: cases.length, helperBacked}));
assert.deepEqual(helperBacked, [], 'integer forms must use the direct generated path');
"#)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        validation.status.success(),
        "generated integer forms failed validation or direct import assertions:\n{}\n{}",
        String::from_utf8_lossy(&validation.stdout),
        String::from_utf8_lossy(&validation.stderr)
    );
}
