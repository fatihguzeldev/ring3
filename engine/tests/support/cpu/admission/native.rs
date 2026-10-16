#![allow(dead_code)]
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        dbt::{CompileError, CompileLimits},
        x86::decode::decode_one,
    },
    memory::GuestAddress,
    process::EngineInstance,
};

pub struct ColdEntryCase<'a> {
    pub key: u64,
    pub keeper: u64,
    pub resident: bool,
    pub pages: &'a [u32],
    pub expected: CompileError,
}

// cold units stop at their instruction budget; strict whole-unit refusal assertions
// and strict public fetch-fault refusals remain separate at each caller.
pub fn admit_if_deferred(engine: &mut EngineInstance, case: ColdEntryCase<'_>) -> bool {
    if case.expected != CompileError::InstructionLimit {
        return false;
    }
    let root = GuestAddress(u32::from_le_bytes(
        engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .try_into()
            .unwrap(),
    ));
    let mut cursor = root;
    for _ in 0..64 {
        let instruction = decode_one(engine.memory().unwrap(), cursor).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(instruction.code_snapshot())
        );
        cursor = instruction.next_pc();
    }
    let arena = engine.arena().to_vec();
    let generation = engine.generation();
    let replacement = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let keeper = engine.resident_bytes(case.keeper).unwrap().to_vec();
    let keeper_pointer = engine.resident_bytes(case.keeper).unwrap().as_ptr();
    let dispatcher = engine.dispatcher_bytes(case.key).unwrap().to_vec();
    let mapped = engine.memory().unwrap().mapped_pages();
    let pages: Vec<_> = case
        .pages
        .iter()
        .map(|&address| {
            let mut bytes = [0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(address), &mut bytes)
                .unwrap();
            bytes
        })
        .collect();
    let id = if case.resident {
        engine.compile_resident_entries(1, 0).unwrap().get()
    } else {
        u64::from(engine.compile_entries(1, 0).unwrap())
    };
    let module = if case.resident {
        engine.resident_bytes(id).unwrap()
    } else {
        engine.artifact_bytes().unwrap()
    };
    assert!(module.len() <= CompileLimits::default().wasm_bytes);
    assert!(
        module
            .windows(b"finish_exception_detection".len())
            .any(|w| w == b"finish_exception_detection")
    );
    if case.resident {
        engine.guard_resident(case.key, id).unwrap();
        assert_eq!(engine.lookup_resident(root.0).unwrap().get(), id);
        assert_eq!(engine.artifact_bytes().unwrap(), replacement);
        assert_eq!(
            engine.artifact_bytes().unwrap().as_ptr(),
            replacement_pointer
        );
        assert_eq!(engine.generation(), generation);
    } else {
        engine.guard(case.key, id as u32).unwrap();
        assert_eq!(engine.generation(), generation + 1);
        assert_eq!(engine.dispatcher_bytes(case.key).unwrap(), dispatcher);
    }
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert_eq!(engine.resident_bytes(case.keeper).unwrap(), keeper);
    assert_eq!(
        engine.resident_bytes(case.keeper).unwrap().as_ptr(),
        keeper_pointer
    );
    engine.guard_resident(case.key, case.keeper).unwrap();
    for (&address, expected) in case.pages.iter().zip(pages) {
        let mut actual = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(address), &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
    }
    true
}

pub fn assert_oversized_explicit(
    engine: &mut EngineInstance,
    root: u32,
    length: u32,
    resident: bool,
) {
    use ring3_engine::{cpu::dbt::RegistryError, process::HostError};
    assert_eq!(root & 0xfff, 0);
    assert!(length < 0xe00);
    let page = root & !0xfff;
    let (generation, keeper) = seed_keeper(engine, page);
    let replacement = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let retained = engine.resident_bytes(keeper).unwrap().to_vec();
    let retained_pointer = engine.resident_bytes(keeper).unwrap().as_ptr();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(page), 4096)
        .unwrap();
    let mapped = engine.memory().unwrap().mapped_pages();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[root.to_le_bytes(), length.to_le_bytes()].concat());
    let arena = engine.arena().to_vec();
    let error = if resident {
        engine.compile_resident(1).unwrap_err()
    } else {
        engine.compile(1).unwrap_err()
    };
    assert_eq!(
        error,
        if resident {
            HostError::Resident(RegistryError::Compile(CompileError::WasmLimit))
        } else {
            HostError::Compile(CompileError::WasmLimit)
        }
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.artifact_bytes().unwrap(), replacement);
    assert_eq!(
        engine.artifact_bytes().unwrap().as_ptr(),
        replacement_pointer
    );
    assert_eq!(engine.resident_bytes(keeper).unwrap(), retained);
    assert_eq!(
        engine.resident_bytes(keeper).unwrap().as_ptr(),
        retained_pointer
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    engine.guard(engine.key(), generation).unwrap();
    engine.guard_resident(engine.key(), keeper).unwrap();
}

fn seed_keeper(engine: &mut EngineInstance, page: u32) -> (u32, u64) {
    let keep = page + 0xe00;
    engine.protect(page, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 3]
        .copy_from_slice(&[0x90, 0xeb, 0]);
    engine.upload(keep, 3).unwrap();
    engine.protect(page, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[keep.to_le_bytes(), 3_u32.to_le_bytes()].concat());
    let generation = engine.compile(1).unwrap();
    let keeper = engine.compile_resident(1).unwrap().get();
    (generation, keeper)
}

pub fn assert_fresh_cold_limit(
    engine: &mut EngineInstance,
    root: u32,
    length: u32,
    resident: bool,
) {
    assert_eq!(root & 0xfff, 0);
    assert!(length < 0xe00);
    let page = root & !0xfff;
    let (_, keeper) = seed_keeper(engine, page);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(page), 4096)
        .unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[root.to_le_bytes(), length.to_le_bytes()].concat());
    let key = engine.key();
    assert!(admit_if_deferred(
        engine,
        ColdEntryCase {
            key,
            keeper,
            resident,
            pages: &[],
            expected: CompileError::InstructionLimit
        }
    ));
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
}
