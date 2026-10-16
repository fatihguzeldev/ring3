#![allow(dead_code)]
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        dbt::{CompileError, CompileLimits},
        x86::decode::decode_one,
    },
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

pub struct ColdEntryCase<'a> {
    pub key: u64,
    pub keeper: u64,
    pub resident: bool,
    pub pages: &'a [u32],
    pub expected: CompileError,
}

// a cold entry can shorten its suffix to fit; an explicit region keeps its whole extent.
pub fn assert_entry_byte_budget(
    memory: &ring3_engine::memory::AddressSpace,
    spec: ring3_engine::cpu::dbt::BlockSpec,
    limits: CompileLimits,
    result: Result<ring3_engine::cpu::dbt::CompiledRegion, CompileError>,
) {
    use ring3_engine::cpu::dbt::{compile_entry_region, prepare_region};
    let full_limits = CompileLimits {
        wasm_bytes: CompileLimits::default().wasm_bytes,
        ..limits
    };
    let prepared = prepare_region(memory, &[spec], full_limits).unwrap();
    let full = compile_entry_region(memory, &[spec.entry], full_limits).unwrap();
    assert_eq!(full.metadata().instructions, prepared.instruction_count());
    let snapshot = memory
        .snapshot_code(spec.entry, spec.byte_length as usize)
        .unwrap();
    let full_bytes = full.wasm_bytes(memory).unwrap();
    if limits.wasm_bytes == 1 {
        assert_eq!(result.unwrap_err(), CompileError::WasmLimit);
    } else {
        let shortened = result.expect("cold entry must retain a bounded executable prefix");
        let metadata = shortened.metadata();
        let bytes = shortened.wasm_bytes(memory).unwrap();
        assert_eq!(metadata.blocks, full.metadata().blocks);
        assert!(metadata.instructions > 0);
        assert!(bytes.len() <= limits.wasm_bytes);
        if limits.wasm_bytes < full_bytes.len() {
            assert!(metadata.instructions < full.metadata().instructions);
        } else {
            assert_eq!(metadata.instructions, full.metadata().instructions);
            assert_eq!(bytes, full_bytes);
        }
    }
    assert!(memory.is_code_current(&snapshot));
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
    let mut code_snapshots = Vec::with_capacity(64);
    for _ in 0..64 {
        let instruction = decode_one(engine.memory().unwrap(), cursor).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(instruction.code_snapshot())
        );
        code_snapshots.push(
            engine
                .memory()
                .unwrap()
                .snapshot_code(cursor, usize::from(instruction.length()))
                .unwrap(),
        );
        cursor = instruction.next_pc();
    }
    let arena = engine.arena().to_vec();
    let generation = engine.generation();
    let replacement = engine.artifact_bytes().unwrap().to_vec();
    let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let keeper = engine.resident_bytes(case.keeper).unwrap().to_vec();
    let keeper_pointer = engine.resident_bytes(case.keeper).unwrap().as_ptr();
    let dispatcher = engine.dispatcher_bytes(case.key).map(<[u8]>::to_vec);
    let installed: Vec<_> = case
        .pages
        .iter()
        .filter_map(|&address| {
            engine
                .lookup_installed_resident(case.key, address)
                .ok()
                .map(|receipt| (address, receipt))
        })
        .collect();
    let mapped = engine.memory().unwrap().mapped_pages();
    let pages: Vec<_> = case
        .pages
        .iter()
        .map(|&address| {
            let memory = engine.memory().unwrap();
            let address = GuestAddress(address);
            let access = [Access::Read, Access::Write, Access::Execute]
                .map(|access| memory.resolve(address, access));
            let mut read = [0; 4096];
            let read = memory.read(address, &mut read).map(|()| read);
            let mut fetched = [0; 4096];
            let fetched = memory.fetch(address, &mut fetched).map(|()| fetched);
            (access, read, fetched)
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
    }
    assert_eq!(
        engine.dispatcher_bytes(case.key).map(<[u8]>::to_vec),
        dispatcher
    );
    for (address, receipt) in installed {
        assert_eq!(
            engine.lookup_installed_resident(case.key, address).unwrap(),
            receipt
        );
    }
    assert!(
        code_snapshots
            .iter()
            .all(|snapshot| engine.memory().unwrap().is_code_current(snapshot))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert_eq!(engine.resident_bytes(case.keeper).unwrap(), keeper);
    assert_eq!(
        engine.resident_bytes(case.keeper).unwrap().as_ptr(),
        keeper_pointer
    );
    engine.guard_resident(case.key, case.keeper).unwrap();
    for (&address, (access, read, fetched)) in case.pages.iter().zip(pages) {
        let memory = engine.memory().unwrap();
        let address = GuestAddress(address);
        assert_eq!(
            [Access::Read, Access::Write, Access::Execute]
                .map(|access| memory.resolve(address, access)),
            access
        );
        let mut actual_read = [0; 4096];
        assert_eq!(
            memory.read(address, &mut actual_read).map(|()| actual_read),
            read
        );
        let mut actual_fetch = [0; 4096];
        assert_eq!(
            memory
                .fetch(address, &mut actual_fetch)
                .map(|()| actual_fetch),
            fetched
        );
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

// preserve the original oversized bank refusal and admit each operation at its original PC.
pub fn admit_dense_bank(
    bytes: &[u8],
    starts: &[u32],
    key: u64,
    mut admit: impl FnMut(&[u8], &[u32]) -> usize,
) -> usize {
    assert!(bytes.ends_with(&[0xeb, 0]));
    let root = starts[0];
    assert_eq!(starts.last(), Some(&(root + bytes.len() as u32 - 2)));
    for resident in [false, true] {
        let mut engine = EngineInstance::new(1, key).unwrap();
        engine.map(root, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(root, bytes.len() as u32).unwrap();
        assert_oversized_explicit(&mut engine, root, bytes.len() as u32, resident);
    }
    let mut covered = 0;
    let mut covered_bytes = 0;
    let mut profiles = 0;
    for group in starts[..starts.len() - 1].chunks(8) {
        let first = (group[0] - root) as usize;
        let end = starts[covered + group.len()];
        let last = (end - root) as usize;
        let mut chunk = bytes[first..last].to_vec();
        chunk.extend_from_slice(&bytes[bytes.len() - 2..]);
        let mut boundaries = group.to_vec();
        boundaries.push(end);
        assert_eq!(&chunk[..chunk.len() - 2], &bytes[first..last]);
        profiles += admit(&chunk, &boundaries);
        covered += group.len();
        covered_bytes += last - first;
    }
    assert_eq!(covered, starts.len() - 1);
    assert_eq!(covered_bytes, bytes.len() - 2);
    profiles
}
