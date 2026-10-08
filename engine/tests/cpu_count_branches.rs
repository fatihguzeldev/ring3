use ring3_engine::process::EngineInstance;

const PC: u32 = 0x1000;

fn admit(opcode: u8) {
    let mut engine = EngineInstance::new(1, 0x6162_6364_6566_6768).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..142].copy_from_slice(&[opcode, 0]);
    engine.upload(PC, 2).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[140..148]
        .copy_from_slice(&[PC.to_le_bytes(), 2_u32.to_le_bytes()].concat());
    engine.compile(1).expect("bare count branch must compile");
}

#[test]
fn loopne_admits_in_bound_engine() {
    admit(0xe0);
}

#[test]
fn loope_admits_in_bound_engine() {
    admit(0xe1);
}

#[test]
fn loop_admits_in_bound_engine() {
    admit(0xe2);
}

#[test]
fn jecxz_admits_in_bound_engine() {
    admit(0xe3);
}

use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{CountBranchKind, Operation},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const FORMS: [(u8, CountBranchKind); 4] = [
    (0xe0, CountBranchKind::LoopNotEqual),
    (0xe1, CountBranchKind::LoopEqual),
    (0xe2, CountBranchKind::Loop),
    (0xe3, CountBranchKind::EcxZero),
];

fn code(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let range = PageRange::new(GuestAddress(base), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn descriptors(engine: &mut EngineInstance, specs: &[(u32, u32)]) {
    let bytes: Vec<_> = specs
        .iter()
        .flat_map(|&(pc, size)| [pc, size].into_iter().flat_map(u32::to_le_bytes))
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
}

fn bound(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
    count: u32,
) -> Result<Option<u64>, ring3_engine::process::HostError> {
    if resident {
        Ok(Some(
            if entries {
                engine.compile_resident_entries(count, 0)?
            } else {
                engine.compile_resident(count)?
            }
            .get(),
        ))
    } else {
        if entries {
            engine.compile_entries(count, 0)?;
        } else {
            engine.compile(count)?;
        }
        Ok(None)
    }
}

fn bytes(engine: &EngineInstance, id: Option<u64>) -> Vec<u8> {
    match id {
        Some(id) => engine.resident_bytes(id).unwrap().to_vec(),
        None => engine.artifact_bytes().unwrap().to_vec(),
    }
}

#[test]
fn typed_rel8_targets_prefixes_and_fetch_boundaries_are_exact() {
    for (opcode, kind) in FORMS {
        for pc in [PC, u32::MAX - 1] {
            for raw in [0x80_u8, 0xfe, 0, 0x7f] {
                let memory = code(&[opcode, raw], pc);
                let instruction = decode_one(&memory, GuestAddress(pc)).unwrap();
                let next = pc.wrapping_add(2);
                let target = next.wrapping_add(raw as i8 as u32);
                assert_eq!(
                    instruction.operation(),
                    &Operation::CountBranch {
                        kind,
                        target: GuestAddress(target)
                    }
                );
                assert_eq!(
                    (instruction.length(), instruction.next_pc()),
                    (2, GuestAddress(next))
                );
                assert!(memory.is_code_current(instruction.code_snapshot()));
            }
        }
        for (prefix, feature) in [
            (0x66, UnsupportedFeature::Opcode),
            (0x67, UnsupportedFeature::Opcode),
            (0xf2, UnsupportedFeature::Opcode),
            (0xf3, UnsupportedFeature::Opcode),
            (0x64, UnsupportedFeature::Segment),
        ] {
            assert_eq!(
                decode_one(&code(&[prefix, opcode, 0], PC), GuestAddress(PC)).unwrap_err(),
                DecodeError::Unsupported(feature)
            );
        }
        assert_eq!(
            decode_one(&code(&[0xf0, opcode, 0], PC), GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let mut memory = code(&[opcode], 0x1fff);
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1fff)),
            Err(DecodeError::MemoryFault { length: 2, .. })
        ));
        let second = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(second, Permissions::READ).unwrap();
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1fff)),
            Err(DecodeError::MemoryFault { length: 2, .. })
        ));
        memory.protect(second, Permissions::EXECUTE).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1fff)).unwrap().length(),
            2
        );
        memory
            .protect(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1fff)),
            Err(DecodeError::MemoryFault { length: 1, .. })
        ));
        assert!(matches!(
            decode_one(&code(&[opcode], u32::MAX), GuestAddress(u32::MAX)),
            Err(DecodeError::MemoryFault { length: 2, .. })
        ));
    }
}

#[test]
fn six_profiles_validate_modules_and_entry_scan_stops_at_branch() {
    let mut modules = Vec::new();
    for (opcode, _) in FORMS {
        let memory = code(&[opcode, 0, 0x0f, 0x0b], PC);
        let spec = BlockSpec {
            entry: GuestAddress(PC),
            byte_length: 2,
        };
        for module in [
            compile_region(&memory, &[spec], CompileLimits::default()).unwrap(),
            compile_entry_region(&memory, &[GuestAddress(PC)], CompileLimits::default()).unwrap(),
        ] {
            assert_eq!(
                (module.metadata().blocks, module.metadata().instructions),
                (1, 1)
            );
            modules.push((0_u8, module.wasm_bytes(&memory).unwrap().to_vec()));
        }
        assert!(matches!(
            compile_region(
                &memory,
                &[BlockSpec {
                    byte_length: 4,
                    ..spec
                }],
                CompileLimits::default()
            ),
            Err(CompileError::Instruction {
                pc: GuestAddress(PC),
                cause: InstructionError::InvalidBlockEnd
            })
        ));
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, 0x6162_6364_6566_6768).unwrap();
                engine.map(PC, 1, 7).unwrap();
                upload(&mut engine, PC, &[opcode, 0, 0x0f, 0x0b]);
                engine.protect(PC, 1, 4).unwrap();
                descriptors(&mut engine, &[(PC, 2)]);
                let id = bound(&mut engine, resident, entries, 1).unwrap();
                modules.push((if resident { 2 } else { 1 }, bytes(&engine, id)));
            }
        }
    }
    let input: Vec<u8> = modules
        .into_iter()
        .flat_map(|(mode, bytes)| {
            [
                vec![mode],
                (bytes.len() as u32).to_le_bytes().to_vec(),
                bytes,
            ]
            .concat()
        })
        .collect();
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'),b=require('node:fs').readFileSync(0);let at=0,n=0;
while(at<b.length){const mode=b[at++],size=b.readUInt32LE(at);at+=4;const m=new WebAssembly.Module(b.subarray(at,at+size));at+=size;
assert.deepEqual(WebAssembly.Module.imports(m),[{module:'env',name:'memory',kind:'memory'},...(mode?[{module:'ring3',name:mode===2?'guard_resident':'guard',kind:'function'}]:[])]);
assert.deepEqual(WebAssembly.Module.exports(m),[{name:'run',kind:'function'}]);n++;}assert.equal(n,24);
"#]).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn late_refusal_keeps_arena_owner_and_source_currency() {
    for resident in [false, true] {
        let mut engine = EngineInstance::new(2, 0x6162_6364_6566_6768).unwrap();
        for (pc, code) in [(PC, &[0xe2, 0][..]), (0x3000, &[0x90, 0x67, 0xe2, 0][..])] {
            engine.map(pc, 1, 7).unwrap();
            upload(&mut engine, pc, code);
            engine.protect(pc, 1, 4).unwrap();
        }
        descriptors(&mut engine, &[(PC, 2)]);
        let id = bound(&mut engine, resident, false, 1).unwrap();
        let module = bytes(&engine, id);
        descriptors(&mut engine, &[(0x3000, 4)]);
        let before = engine.arena().to_vec();
        let error = format!("{:?}", bound(&mut engine, resident, false, 1).unwrap_err());
        assert!(
            error.contains("GuestAddress(12289)") && error.contains("Unsupported(Opcode)"),
            "{error}"
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(bytes(&engine, id), module);
        engine.protect(PC, 1, 7).unwrap();
        upload(&mut engine, PC, &[0xe2, 0]);
        if let Some(id) = id {
            assert!(engine.resident_bytes(id).is_err());
        } else {
            assert!(engine.artifact_bytes().is_err());
        }
    }
}

#[test]
fn default_instruction_and_block_caps_remain_bounded() {
    for nops in [63, 64] {
        let memory = code(&[vec![0x90; nops], vec![0xe2, 0]].concat(), PC);
        let spec = BlockSpec {
            entry: GuestAddress(PC),
            byte_length: (nops + 2) as u32,
        };
        assert_eq!(
            compile_region(&memory, &[spec], CompileLimits::default()).is_ok(),
            nops == 63
        );
        assert_eq!(
            compile_entry_region(&memory, &[GuestAddress(PC)], CompileLimits::default()).is_ok(),
            nops == 63
        );
    }
    let memory = code(&[0xe2, 0].repeat(9), PC);
    let specs: Vec<_> = (0..9)
        .map(|index| BlockSpec {
            entry: GuestAddress(PC + index * 2),
            byte_length: 2,
        })
        .collect();
    assert_eq!(
        compile_region(&memory, &specs[..8], CompileLimits::default())
            .unwrap()
            .metadata()
            .blocks,
        8
    );
    assert!(matches!(
        compile_region(&memory, &specs, CompileLimits::default()),
        Err(CompileError::InvalidBlocks)
    ));
    assert!(matches!(
        compile_region(
            &memory,
            &specs[..1],
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            }
        ),
        Err(CompileError::WasmLimit)
    ));
}
