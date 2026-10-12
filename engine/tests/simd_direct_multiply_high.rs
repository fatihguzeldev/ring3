use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        Register32,
        decode::decode_one,
        ir::{EffectiveAddress, Operation},
        simd::{PackedKind, SimdKind, SimdOperand, SimdRegister},
    },
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

#[test]
fn all_high_multiply_forms_validate_without_semantic_helper_imports() {
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/direct-multiply-high-fixtures")
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
    for (kind, opcode) in [
        (PackedKind::MultiplyHighSigned, 0xe5),
        (PackedKind::MultiplyHighUnsigned, 0xe4),
    ] {
        for vector_bytes in [8, 16] {
            let register = |index| {
                if vector_bytes == 8 {
                    SimdRegister::Mmx(index)
                } else {
                    SimdRegister::Xmm(index)
                }
            };
            for form in 0..3 {
                let mut bytes = Vec::new();
                if vector_bytes == 16 {
                    bytes.push(0x66);
                }
                let (modrm, source, memory_bytes) = match form {
                    0 => (0xf7, SimdOperand::Register(register(7)), 0),
                    1 => (0xf6, SimdOperand::Register(register(6)), 0),
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
                let length = bytes.len();
                bytes.extend([0xeb, 0xfe]);
                let mut engine = EngineInstance::new(1, 0x4849_4d55_4c33_3201).unwrap();
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
                    panic!("expected a typed high-half multiply");
                };
                assert_eq!(operation.kind, SimdKind::Packed(kind));
                assert_eq!(operation.destination, SimdOperand::Register(register(6)));
                assert_eq!(operation.source, source);
                assert_eq!(operation.lane_bytes, 2);
                assert_eq!(operation.vector_bytes, vector_bytes);
                assert_eq!(operation.memory_bytes, memory_bytes);
                assert_eq!(operation.immediate, None);
                assert_eq!(operation.aligned, form == 2 && vector_bytes == 16);
                assert!(!operation.scalar);
                assert_eq!(operation.implicit_address, None);
                assert_eq!(engine.arena(), before);

                engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
                    .copy_from_slice(&0x2000_u32.to_le_bytes());
                if form != 2 {
                    engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
                        .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                }
                let before = engine.arena().to_vec();
                let module = if form == 2 {
                    let id = engine.compile_resident_entries(1, 0).unwrap().get();
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.compile(1).unwrap();
                    engine.artifact_bytes().unwrap()
                };
                let name = format!("multiply-high-{}.wasm", modules.len());
                fs::write(output.join(&name), module).unwrap();
                modules.push(format!("[\"{name}\",{memory_bytes}]"));
                assert_eq!(engine.arena(), before);
                let memory = engine.memory().unwrap();
                assert_eq!(memory.mapped_pages(), 1);
                for address in [0, 0x2000, 0x5000] {
                    assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
                }
            }
        }
    }
    assert_eq!(modules.len(), 12);
    fs::write(
        output.join("modules.json"),
        format!("[{}]", modules.join(",")),
    )
    .unwrap();
    let validation = Command::new("node")
        .arg("-e")
        .arg(
            r#"
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
assert.deepEqual(helperBacked, [], 'high-half multiplies must use the direct generated path');
"#,
        )
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        validation.status.success(),
        "generated high-half multiplies failed validation or direct import assertions:\n{}\n{}",
        String::from_utf8_lossy(&validation.stdout),
        String::from_utf8_lossy(&validation.stderr)
    );
}
