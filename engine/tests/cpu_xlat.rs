use ring3_engine::process::EngineInstance;

const PC: u32 = 0x1000;

#[test]
fn xlatb_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[0xd7, 0xeb, 0]);
    engine.upload(PC, 3).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[140..148]
        .copy_from_slice(&[PC.to_le_bytes(), 3_u32.to_le_bytes()].concat());
    engine.compile(1).expect("bare XLATB must compile");
}

use ring3_engine::{
    cpu::{
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{decode::decode_one, ir::Operation},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn exact_xlat_fetch_prefix_and_standalone_boundaries() {
    for pc in [PC, 0x1fff, u32::MAX] {
        let memory = memory(pc, &[0xd7]);
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::TranslateByte);
        assert_eq!(decoded.length(), 1);
        assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(1)));
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        assert!(decode_one(&memory(PC, &[prefix, 0xd7]), GuestAddress(PC)).is_err());
    }
    assert!(decode_one(&AddressSpace::new(1).unwrap(), GuestAddress(PC)).is_err());
    assert!(matches!(
        compile_region(
            &memory(PC, &[0xd7, 0xeb, 0]),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 3,
            }],
            CompileLimits::default()
        ),
        Err(CompileError::Instruction {
            pc: GuestAddress(PC),
            cause: InstructionError::BackendUnsupported
        })
    ));
    assert!(matches!(
        compile_entry_region(
            &memory(PC, &[0xd7, 0xeb, 0]),
            &[GuestAddress(PC)],
            CompileLimits::default()
        ),
        Err(CompileError::Instruction {
            pc: GuestAddress(PC),
            cause: InstructionError::BackendUnsupported
        })
    ));
}

fn compile(
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

fn module(engine: &EngineInstance, id: Option<u64>) -> Vec<u8> {
    match id {
        Some(id) => engine.resident_bytes(id).unwrap().to_vec(),
        None => engine.artifact_bytes().unwrap().to_vec(),
    }
}

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(PC, bytes.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, length: usize, entries: bool) {
    let bytes = if entries {
        PC.to_le_bytes().to_vec()
    } else {
        [PC.to_le_bytes(), (length as u32).to_le_bytes()].concat()
    };
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(&bytes);
}

#[test]
fn all_four_bound_profiles_validate_only_guard_and_read8_without_data_access() {
    let mut input = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&[0xd7, 0xeb, 0]);
            describe(&mut engine, 3, entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, resident, entries, 1).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            let bytes = module(&engine, id);
            input.push(u8::from(resident));
            input.extend((bytes.len() as u32).to_le_bytes());
            input.extend(bytes);
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'), b=require('node:fs').readFileSync(0); let at=0,count=0;
while(at<b.length){const resident=b[at++],n=b.readUInt32LE(at);at+=4;
 const module=new WebAssembly.Module(b.subarray(at,at+n));at+=n;
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},
 ...[resident?'guard_resident':'guard','read8'].map(name=>({module:'ring3',name,kind:'function'}))]);
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
 const run=new WebAssembly.Instance(module,{env:{memory:new WebAssembly.Memory({initial:1})},
 ring3:{guard(){return 0;},guard_resident(){return 0;},read8(){return 0;}}}).exports.run;
 assert.equal(run.length,4);count++;
}assert.equal(count,4);
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn late_prefix_failure_preserves_previous_owner_and_complete_arena() {
    for resident in [false, true] {
        let mut engine = EngineInstance::new(2, 0x5152_5354_5556_5758).unwrap();
        for (pc, bytes) in [
            (PC, &[0x90, 0xeb, 0][..]),
            (0x3000, &[0x90, 0xd7, 0x66, 0xd7][..]),
        ] {
            engine.map(pc, 1, 7).unwrap();
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
            engine.upload(pc, bytes.len() as u32).unwrap();
            engine.protect(pc, 1, 4).unwrap();
        }
        describe(&mut engine, 3, false);
        let id = compile(&mut engine, resident, false, 1).unwrap();
        let old = module(&engine, id);
        engine.arena_mut().unwrap()[140..148]
            .copy_from_slice(&[0x3000_u32.to_le_bytes(), 4_u32.to_le_bytes()].concat());
        let before = engine.arena().to_vec();
        let error = compile(&mut engine, resident, false, 1).unwrap_err();
        assert!(format!("{error:?}").contains("GuestAddress(12290)"));
        assert_eq!(engine.arena(), before);
        assert_eq!(module(&engine, id), old);
    }
}

#[test]
fn xlat_instruction_and_block_caps_keep_the_existing_limits() {
    for resident in [false, true] {
        for nops in [62, 63] {
            let bytes = [vec![0x90; nops], vec![0xd7, 0xeb, 0]].concat();
            let mut engine = code(&bytes);
            describe(&mut engine, bytes.len(), false);
            assert_eq!(compile(&mut engine, resident, false, 1).is_ok(), nops == 62);
        }
        let bytes = [0xd7, 0xeb, 0].repeat(8);
        let mut engine = code(&bytes);
        let descriptors: Vec<u8> = (0..8_u32)
            .flat_map(|i| [PC + i * 3, 3].into_iter().flat_map(u32::to_le_bytes))
            .collect();
        engine.arena_mut().unwrap()[140..204].copy_from_slice(&descriptors);
        let id = compile(&mut engine, resident, false, 8).unwrap();
        let old = module(&engine, id);
        let before = engine.arena().to_vec();
        assert!(compile(&mut engine, resident, false, 9).is_err());
        assert_eq!(engine.arena(), before);
        assert_eq!(module(&engine, id), old);
    }
}
