use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admitted(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 1).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    descriptor[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("bare memory bit mutation must admit");
    engine
}

#[test]
fn baseline_memory_bt_and_store32_modules_are_preserved() {
    for (name, bytes) in [
        ("bt", &[0x0f, 0xa3, 0x0b, 0xeb, 0][..]),
        ("store", &[0x89, 0x03, 0xeb, 0][..]),
    ] {
        let engine = admitted(bytes);
        if let Some(directory) = std::env::var_os("RING3_BIT_MODULE_OUTPUT") {
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::path::Path::new(&directory).join(format!("{name}.wasm")))
                .unwrap();
            output.write_all(engine.artifact_bytes().unwrap()).unwrap();
        }
    }
}

#[test]
fn memory_bts_admits_existing_public_api() {
    admitted(&[0x0f, 0xab, 0x0b, 0xeb, 0]);
    admitted(&[0x0f, 0xba, 0x2b, 0xff, 0xeb, 0]);
}

#[test]
fn memory_btr_admits_existing_public_api() {
    admitted(&[0x0f, 0xb3, 0x0b, 0xeb, 0]);
    admitted(&[0x0f, 0xba, 0x33, 0xff, 0xeb, 0]);
}

#[test]
fn memory_btc_admits_existing_public_api() {
    admitted(&[0x0f, 0xbb, 0x0b, 0xeb, 0]);
    admitted(&[0x0f, 0xba, 0x3b, 0xff, 0xeb, 0]);
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BitIndex, BitTestKind, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [BitTestKind; 3] = [
    BitTestKind::Set,
    BitTestKind::Reset,
    BitTestKind::Complement,
];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}
fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(base, pages, 4).unwrap();
    engine
}
fn descriptor(engine: &mut EngineInstance, pc: u32, bytes: usize, entries: bool) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(
        &[
            pc.to_le_bytes(),
            if entries { 0 } else { bytes as u32 }.to_le_bytes(),
        ]
        .concat(),
    );
}
fn bound(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}
fn encoded(kind: BitTestKind, index: BitIndex, tail: &[u8]) -> Vec<u8> {
    let (opcode, field) = match kind {
        BitTestKind::Set => (0xab, 5),
        BitTestKind::Reset => (0xb3, 6),
        BitTestKind::Complement => (0xbb, 7),
        BitTestKind::Test => unreachable!(),
    };
    let mut bytes = vec![
        0x0f,
        if matches!(index, BitIndex::Immediate(_)) {
            0xba
        } else {
            opcode
        },
    ];
    bytes.extend(tail);
    match index {
        BitIndex::Register(source) => bytes[2] |= (source as u8) << 3,
        BitIndex::Immediate(raw) => {
            bytes[2] |= field << 3;
            bytes.push(raw);
        }
    }
    bytes
}
fn forms() -> Vec<(Vec<u8>, EffectiveAddress)> {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esp};
    [
        (vec![0x03], Some(Ebx), None, 1, 0),
        (vec![0x00], Some(Eax), None, 1, 0),
        (vec![0x02], Some(Edx), None, 1, 0),
        (vec![0x04, 0x24], Some(Esp), None, 1, 0),
        (vec![0x44, 0x8f, 0x80], Some(Edi), Some(Ecx), 4, 0xffff_ff80),
        (vec![0x45, 0x80], Some(Ebp), None, 1, 0xffff_ff80),
        (vec![0x83, 0x10, 0, 0, 0], Some(Ebx), None, 1, 0x10),
        (vec![0x05, 0x10, 0x50, 0, 0], None, None, 1, 0x5010),
    ]
    .into_iter()
    .map(|(tail, base, index, scale, displacement)| {
        (
            tail,
            EffectiveAddress {
                base,
                index,
                scale,
                displacement,
            },
        )
    })
    .collect()
}

#[test]
fn typed_aliases_imports_and_four_bound_profiles_are_exact() {
    let mut modules = Vec::new();
    let mut typed = 0;
    for kind in KINDS {
        let mut rows = Vec::new();
        for index in [
            BitIndex::Register(Register32::Ecx),
            BitIndex::Immediate(255),
        ] {
            for (tail, address) in forms() {
                rows.push((index, tail, address));
            }
        }
        for source in [
            Register32::Eax,
            Register32::Edx,
            Register32::Ebx,
            Register32::Esp,
            Register32::Ebp,
            Register32::Esi,
            Register32::Edi,
        ] {
            rows.push((BitIndex::Register(source), vec![0x03], forms()[0].1));
        }
        for raw in [0, 31, 32] {
            rows.push((BitIndex::Immediate(raw), vec![0x03], forms()[0].1));
        }
        assert_eq!(rows.len(), 26);
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (index, tail, address) in rows {
            let instruction = encoded(kind, index, &tail);
            expected.push((CODE + bytes.len() as u32, instruction.len(), address, index));
            bytes.extend(instruction);
        }
        bytes.extend([0xeb, 0]);
        let engine = fixture(CODE, &bytes);
        for (pc, length, address, index) in expected {
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::MemoryBitMutation {
                    kind,
                    address,
                    index
                }
            );
            assert_eq!(
                (decoded.length() as usize, decoded.next_pc()),
                (length, GuestAddress(pc + length as u32))
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            typed += 1;
        }
        let refused = Some(CompileError::Instruction {
            pc: GuestAddress(CODE),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32
                }],
                CompileLimits::default()
            )
            .err(),
            refused
        );
        assert_eq!(
            compile_entry_region(
                engine.memory().unwrap(),
                &[GuestAddress(CODE)],
                CompileLimits::default()
            )
            .err(),
            refused
        );
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = fixture(CODE, &bytes);
                descriptor(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let id = bound(&mut engine, resident, entries).unwrap();
                assert_eq!(engine.arena(), before);
                let generated = if resident {
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.artifact_bytes().unwrap()
                };
                modules.push(u8::from(resident));
                modules.extend((generated.len() as u32).to_le_bytes());
                modules.extend(generated);
            }
        }
    }
    assert_eq!(typed, 78);
    let mut child=Command::new("node").args(["-e",r#"
const a=require('node:assert/strict'),b=require('node:fs').readFileSync(0);let at=0,n=0;
function u(p){let x=0,s=0,v;do{v=b[p.i++];x|=(v&127)<<s;s+=7;}while(v&128);return x>>>0;}
while(at<b.length){const resident=b[at++],size=b.readUInt32LE(at);at+=4;const end=at+size;
a.ok(WebAssembly.validate(b.subarray(at,end)));const m=new WebAssembly.Module(b.subarray(at,end));
a.deepEqual(WebAssembly.Module.imports(m),[{module:'env',name:'memory',kind:'memory'},
...[resident?'guard_resident':'guard','read32',resident?'store_resident32':'store32'].map(name=>({module:'ring3',name,kind:'function'}))]);
a.deepEqual(WebAssembly.Module.exports(m),[{name:'run',kind:'function'}]);
const p={i:at+8};let found=false;while(p.i<end){const id=b[p.i++],stop=u(p)+p.i;if(id===10){a.equal(u(p),1);u(p);const c=u(p),locals=[];for(let i=0;i<c;i++)locals.push([u(p),b[p.i++]]);a.deepEqual(locals,[[16,127],[1,126],[6,127]]);found=true;}p.i=stop;}
a.ok(found);at=end;n++;}a.equal(n,12);
"#]).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&modules).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn strict_neighbors_fetch_wrap_and_source_currency_are_exact() {
    for kind in KINDS {
        for index in [
            BitIndex::Register(Register32::Ecx),
            BitIndex::Immediate(255),
        ] {
            let instruction = encoded(kind, index, &[0x03]);
            for prefix in [
                0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
            ] {
                let mut bytes = vec![prefix];
                bytes.extend(&instruction);
                let feature = if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
                    UnsupportedFeature::Segment
                } else {
                    UnsupportedFeature::Opcode
                };
                assert_eq!(
                    decode_one(fixture(CODE, &bytes).memory().unwrap(), GuestAddress(CODE))
                        .unwrap_err(),
                    DecodeError::Unsupported(feature)
                );
            }
            for (tail, _) in forms()
                .into_iter()
                .filter(|(t, _)| matches!(t.as_slice(), [0x03] | [0x05, ..] | [0x44, ..]))
            {
                let bytes = encoded(kind, index, &tail);
                for pc in [
                    0x2000 - bytes.len() as u32,
                    u32::MAX - (bytes.len() as u32 - 1),
                    0x1fff,
                ] {
                    let engine = fixture(pc, &bytes);
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                    assert_eq!(
                        (decoded.length() as usize, decoded.next_pc()),
                        (
                            bytes.len(),
                            GuestAddress(pc.wrapping_add(bytes.len() as u32))
                        )
                    );
                }
                for length in 1..bytes.len() {
                    let pc = 0x2000 - length as u32;
                    let mut engine = fixture(pc, &bytes[..length]);
                    for reason in [FaultReason::Unmapped, FaultReason::Permission] {
                        if reason == FaultReason::Permission {
                            engine.map(0x2000, 1, 3).unwrap();
                        }
                        assert_eq!(
                            decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap_err(),
                            DecodeError::MemoryFault {
                                pc: GuestAddress(pc),
                                fault: MemoryFault {
                                    address: GuestAddress(0x2000),
                                    access: Access::Execute,
                                    reason
                                },
                                length: length as u32 + 1
                            }
                        );
                    }
                }
            }
            let mut engine = fixture(CODE, &instruction);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            engine.protect(CODE, 1, 7).unwrap();
            upload(&mut engine, CODE, &[0x90]);
            assert!(
                !engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
        }
    }
}

#[test]
fn caps_and_late_failures_preserve_bound_owners_and_full_arena() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65536
        }
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(CODE, &[0x90]);
            engine.map(KEEP, 1, 7).unwrap();
            upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
            descriptor(&mut engine, KEEP, 3, false);
            let generation = engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let prior = (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
            );
            engine.map(0x5000, 1, 3).unwrap();
            upload(&mut engine, 0x5010, &[1, 0, 0, 0]);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            engine.protect(CODE, 1, 7).unwrap();
            for kind in KINDS {
                for index in [
                    BitIndex::Register(Register32::Ecx),
                    BitIndex::Immediate(255),
                ] {
                    let first = encoded(kind, index, &[0x03]);
                    let late = CODE + first.len() as u32;
                    let mut bytes = first.clone();
                    bytes.push(0x66);
                    bytes.extend(first);
                    upload(&mut engine, CODE, &bytes);
                    descriptor(&mut engine, CODE, bytes.len(), entries);
                    let before = engine.arena().to_vec();
                    let failure = CompileError::Instruction {
                        pc: GuestAddress(late),
                        cause: InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Opcode,
                        )),
                    };
                    let expected = if resident {
                        HostError::Resident(RegistryError::Compile(failure))
                    } else {
                        HostError::Compile(failure)
                    };
                    assert_eq!(bound(&mut engine, resident, entries), Err(expected));
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.generation(), generation);
                    assert_eq!(engine.artifact_bytes().unwrap(), prior.0);
                    assert_eq!(engine.resident_bytes(keep).unwrap(), prior.1);
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                }
            }
        }
    }
    for count in [63, 64] {
        let mut bytes = encoded(
            BitTestKind::Set,
            BitIndex::Register(Register32::Ecx),
            &[0x03],
        );
        bytes.extend(vec![0x90; count - 1]);
        bytes.extend([0xeb, 0]);
        let mut engine = fixture(CODE, &bytes);
        descriptor(&mut engine, CODE, bytes.len(), false);
        if count == 63 {
            engine.compile(1).unwrap();
        } else {
            assert_eq!(
                engine.compile(1),
                Err(HostError::Compile(CompileError::InstructionLimit))
            );
        }
    }
    let mut large = encoded(
        BitTestKind::Set,
        BitIndex::Register(Register32::Ecx),
        &[0x03],
    )
    .repeat(63);
    large.extend([0xeb, 0]);
    let mut engine = fixture(CODE, &large);
    descriptor(&mut engine, CODE, large.len(), false);
    assert_eq!(
        engine.compile(1),
        Err(HostError::Compile(CompileError::WasmLimit))
    );
    let mut bytes = vec![0xcc; 128];
    for at in (0..128).step_by(16) {
        bytes[at..at + 5].copy_from_slice(&[0x0f, 0xab, 0x0b, 0xeb, 0]);
    }
    let mut engine = fixture(CODE, &bytes);
    let descriptors: Vec<u8> = (0..8)
        .flat_map(|i| [(CODE + i * 16).to_le_bytes(), 5_u32.to_le_bytes()].concat())
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine.compile(8).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile(9), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
}
