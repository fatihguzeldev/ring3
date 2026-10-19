use crate::{
    abi::x86::{EXIT_SIZE, STATE_SIZE},
    cpu::{
        dbt::{CompilationTier32, CompileLimits, PreparedResidentReplacement, RegistryError},
        x86::{State32, decode::DecodeAuthority},
    },
    memory::GuestAddress,
    process::{
        EngineInstance,
        calls::call::{PendingCall, PendingOwner},
        cpu::context::ExtendedCpuState,
        execution::cache::{CacheRequest32, CacheRequestKind32, ExecutionConfig32},
    },
    windows::{CallFrame32, CallingConvention32},
};

pub(super) const KEY: u64 = 0x4849_4e54;
pub(super) const ROOTS: [u32; 3] = [0x1000, 0x2000, 0x3000];
pub(super) const TARGETS: [Option<u32>; 3] = [Some(0x2003), Some(0x3000), Some(0x1000)];
pub(super) const STARTS: [&[u32]; 3] = [
    &[0x1000, 0x1003, 0x1005, 0x100b, 0x100f, 0x1200],
    &[0x2000, 0x2003, 0x2006, 0x2008, 0x200e, 0x2012],
    &[0x3000, 0x3003, 0x3005, 0x300b, 0x300f],
];

pub(super) struct Fixture {
    pub engine: EngineInstance,
    pub owners: [CacheRequest32; 3],
}

pub(super) fn limits() -> CompileLimits {
    CompileLimits {
        blocks: 8,
        instructions: 256,
        wasm_bytes: 512 * 1024,
    }
}

fn install(engine: &mut EngineInstance, pc: u32) -> CacheRequest32 {
    let request = engine.prepare_cache_entry(KEY, pc).unwrap();
    assert_eq!(request.kind, CacheRequestKind32::Install);
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    request
}

pub(super) fn pin(engine: &mut EngineInstance, owner: CacheRequest32) {
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        State32 {
            registers: [0, 0, 0, 0, 0x9800, 0, 0, 0],
            eip: owner.pc,
            eflags: 2,
        },
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(owner.unit_id),
        frame,
        state: [0; STATE_SIZE],
        exit: [0; EXIT_SIZE],
        extended: ExtendedCpuState::initial().unwrap(),
    });
}

pub(super) fn fixture() -> Fixture {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    for pc in ROOTS {
        engine.map(pc, 1, 7).unwrap();
    }
    for pc in [0x8000, 0x9000] {
        engine.map(pc, 1, 3).unwrap();
    }
    // a's hot and cold paths reach a local jmp esi. no static a -> b target exists.
    // b2003 is a real old interior start; entering it must keep b2000 available too.
    for (pc, bytes) in [
        (
            0x1000,
            &[0x83, 0xc0, 1, 0x85, 0xff, 0x0f, 0x84, 0xf5, 1, 0, 0][..],
        ),
        (0x100b, &[0x83, 0x43, 4, 17, 0xe9, 0xec, 1, 0, 0][..]),
        (0x1200, &[0xff, 0xe6][..]),
        (
            0x2000,
            &[
                0x83, 0xc5, 2, 0x83, 3, 1, 0x85, 0xff, 0x0f, 0x84, 0xf2, 0x0f, 0, 0,
            ][..],
        ),
        (0x200e, &[0x83, 0x43, 8, 33, 0xe9, 0xe9, 0x0f, 0, 0][..]),
        (
            0x3000,
            &[
                0x83, 0xc2, 3, 0x85, 0xff, 0x0f, 0x84, 0xf5, 0xdf, 0xff, 0xff,
            ][..],
        ),
        (
            0x300b,
            &[0x83, 0x43, 12, 49, 0xe9, 0xec, 0xdf, 0xff, 0xff][..],
        ),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), bytes)
            .unwrap();
    }
    engine
        .configure_execution_with_limits(
            ExecutionConfig32 {
                units: 8,
                wasm_bytes: 4 * 1024 * 1024,
                promotion_hits: 8,
            },
            limits(),
        )
        .unwrap();
    let mut owners = ROOTS.map(|pc| install(&mut engine, pc));
    for owner in owners {
        assert_eq!(owner.tier, CompilationTier32::Baseline);
    }
    for index in 0..3 {
        // same real runtime promotion path as the existing whole-owner fixture.
        // the saved call is only fixture pin setup, never fabricated tier1 authority.
        pin(&mut engine, owners[(index + 1) % 3]);
        for _ in 0..8 {
            engine.lookup_installed_resident(KEY, ROOTS[index]).unwrap();
        }
        let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
        assert_eq!(
            (request.kind, request.victim_id),
            (CacheRequestKind32::Evict, owners[index].unit_id)
        );
        engine
            .acknowledge_cache_eviction(KEY, request.serial, request.victim_id, request.victim_slot)
            .unwrap();
        let promoted = install(&mut engine, ROOTS[index]);
        assert_eq!(promoted.tier, CompilationTier32::Optimized);
        assert_ne!(promoted.unit_id, owners[index].unit_id);
        owners[index] = promoted;
        engine.pending_call = None;
    }
    let fixture = Fixture { engine, owners };
    let memory = fixture.engine.memory.as_ref().unwrap();
    let registry = fixture.engine.resident.as_ref().unwrap();
    for (index, owner) in fixture.owners.iter().enumerate() {
        let region = registry.get_raw(memory, owner.unit_id).unwrap();
        assert_eq!(region.tier(), CompilationTier32::Optimized);
        assert_eq!(
            (region.metadata().blocks, region.metadata().instructions),
            ([3, 2, 2][index], STARTS[index].len())
        );
        for &pc in STARTS[index] {
            assert!(region.contains_instruction(pc));
            assert!(region.contains_executable(pc));
        }
    }
    assert_old_lookup(&fixture);
    fixture
}

pub(super) fn prepare(
    fixture: &mut Fixture,
    configured: CompileLimits,
    targets: [Option<u32>; 3],
    excluded: Option<u64>,
    gate: Option<u32>,
) -> Result<Option<PreparedResidentReplacement>, RegistryError> {
    let cs = fixture.engine.code_fetch_context().unwrap();
    let owners = fixture.owners;
    fixture
        .engine
        .resident
        .as_mut()
        .unwrap()
        .prepare_hinted_replacement(
            fixture.engine.memory.as_ref().unwrap(),
            GuestAddress(ROOTS[0]),
            configured,
            KEY,
            cs,
            DecodeAuthority::default(),
            &|id| owners.iter().any(|owner| owner.unit_id == id) && excluded != Some(id),
            &|pc| gate == Some(pc.0),
            &|id| {
                owners
                    .iter()
                    .position(|owner| owner.unit_id == id)
                    .and_then(|i| targets[i])
                    .map(GuestAddress)
            },
        )
}

pub(super) fn retained(fixture: &Fixture) -> Vec<(u64, usize, usize)> {
    fixture
        .owners
        .iter()
        .map(|owner| {
            // retained view deliberately also works after a source/decode invalidation.
            let region = fixture
                .engine
                .resident
                .as_ref()
                .unwrap()
                .get_retained_raw(fixture.engine.memory.as_ref().unwrap(), owner.unit_id)
                .unwrap();
            (
                owner.unit_id,
                region as *const _ as usize,
                region.retained_wasm_byte_len(),
            )
        })
        .collect()
}

pub(super) fn assert_old_lookup(fixture: &Fixture) {
    let cs = fixture.engine.code_fetch_context().unwrap();
    for (owner, starts) in fixture.owners.iter().zip(STARTS) {
        for &pc in starts {
            assert_eq!(
                fixture
                    .engine
                    .resident
                    .as_ref()
                    .unwrap()
                    .lookup_bound(
                        fixture.engine.memory.as_ref().unwrap(),
                        GuestAddress(pc),
                        cs
                    )
                    .unwrap()
                    .get(),
                owner.unit_id
            );
            assert_eq!(
                fixture
                    .engine
                    .resident_installation_by_id(owner.unit_id)
                    .unwrap()
                    .slot,
                owner.slot
            );
        }
    }
}
